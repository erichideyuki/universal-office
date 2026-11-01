//! Porta de docx-layout/src/lines.ts (LineBreaker, nextTab, moreTabs, lineBox) e da escolha de
//! hifenização de hyphenate.ts (hyphenCandidates, chooseHyphenation). Mesmo algoritmo e a
//! mesma ordem das operações de ponto flutuante: as linhas têm de sair idênticas às do TS.
//! Diferença de forma, não de resultado: os `includes` de lista do TS (O(n) por item) viram
//! marcas por índice.
use crate::linedata::*;
use crate::read::js_round;

/// Espaço de largura fixa (en, em, fino, ideográfico…): fora da justificação.
#[inline]
fn fixed_space(cp: u32) -> bool {
    (0x2000..=0x200a).contains(&cp) || cp == 0x3000
}

const FIT_EPS: f64 = 0.001;
const WORD_UNIT: f64 = 20.0 / 4096.0;
const HYPHEN_GAP15: f64 = 421.0;
const HYPHEN_EXPAND15: f64 = 0.4868;
const HYPHEN_ZONE_DEFAULT: f64 = 425.0;

#[inline]
fn to_word_unit(tw: f64) -> f64 {
    js_round(tw / WORD_UNIT) * WORD_UNIT
}

#[derive(Clone, Copy)]
struct TabHit {
    pos: f64,
    tab: Option<Tab>,
}

fn next_tab(x: f64, tabs: &[Tab], default_tab: f64, hanging_stop: Option<f64>) -> TabHit {
    let mut best: Option<TabHit> = None;
    for t in tabs {
        if t.val == TAB_BAR {
            continue;
        }
        if t.pos > x + 0.5 {
            best = Some(TabHit { pos: t.pos, tab: Some(*t) });
            break;
        }
    }
    if let Some(h) = hanging_stop {
        if h > x + 0.5 && best.is_none_or(|b| h < b.pos) {
            best = Some(TabHit { pos: h, tab: None });
        }
    }
    if let Some(b) = best {
        return b;
    }
    let last_custom = tabs.iter().fold(f64::NEG_INFINITY, |m, t| m.max(t.pos));
    let from = x.max(last_custom);
    let dts = if default_tab > 0.0 { default_tab } else { 720.0 };
    let mut pos = ((from / dts + 1e-6).floor() + 1.0) * dts;
    if pos <= x + 0.5 {
        pos += dts;
    }
    TabHit { pos, tab: None }
}

/// Há outro tab depois do glifo `i`, antes da próxima quebra de linha/página?
fn more_tabs(gs: &[Glyph], i: usize) -> bool {
    for q in &gs[i + 1..] {
        if q.kind == Kind::Tab {
            return true;
        }
        if matches!(q.kind, Kind::Lb | Kind::Pb | Kind::Cb) {
            return false;
        }
    }
    false
}

/// Altura/baseline da linha pela regra de entrelinha.
pub fn line_box(asc: f64, desc: f64, p: &Para, text_nat: Option<f64>, grid: Option<f64>) -> (f64, f64) {
    let nat = asc + desc;
    if p.line_rule == RULE_EXACT {
        let h = p.line;
        return (h, h * 0.8);
    }
    if let Some(grid) = grid.filter(|g| *g > 0.0) {
        let g = ((nat - 1.0) / grid).ceil().max(1.0) * grid;
        let g_base = (g - nat) / 2.0 + asc;
        if p.line_rule == RULE_ATLEAST {
            let h = g.max(p.line);
            return (h, h - g + g_base);
        }
        let h = ((grid * p.line) / 240.0).max(g);
        return (h, (h - nat) / 2.0 + asc);
    }
    if p.line_rule == RULE_ATLEAST {
        let h = nat.max(p.line);
        return (h, h - desc);
    }
    let extra = (p.line / 240.0 - 1.0) * text_nat.unwrap_or(nat);
    let h = nat + extra;
    (h, if extra >= 0.0 { asc } else { asc * (h / nat) })
}

struct HyphenUnit {
    i: usize,
    e: usize,
    x: f64,
    right: f64,
    line_start: bool,
    content_end: f64,
    justify: bool,
    spaces_w: f64,
    rest_w: f64,
    n_spaces: u32,
    hyphen_run: i32,
}

fn hyphen_candidates(gs: &[Glyph], i: usize, e: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut letters = 0;
    let mut k = i;
    while k + 1 < e {
        let g = &gs[k];
        if g.kind == Kind::Shy {
            if k > i {
                out.push(k);
            }
            k += 1;
            continue;
        }
        if g.has(F_LETTER) {
            letters += 1;
        }
        if g.has(F_HYPH) && gs.get(k + 1).is_none_or(|n| n.kind != Kind::Shy) && letters >= 2 {
            out.push(k);
        }
        k += 1;
    }
    out
}

fn choose_hyphenation(s: &Settings, opts: &Opts, gs: &[Glyph], u: &HyphenUnit) -> Option<(usize, f64)> {
    let mut cands = hyphen_candidates(gs, u.i, u.e);
    let limit = s.hyph_limit;
    if limit > 0 && u.hyphen_run >= limit {
        cands.retain(|&k| gs[k].kind == Kind::Shy);
    }
    if cands.is_empty() {
        return None;
    }
    let modern = s.compat >= 15;
    let justify = u.justify && modern;
    let hyphen_w = |k: usize| gs[k].hy_w;
    let mut ends = Vec::with_capacity(cands.len());
    let mut x = u.x;
    let mut ci = 0;
    let mut k = u.i;
    while k < u.e && ci < cands.len() {
        let g = &gs[k];
        if g.kind != Kind::Shy {
            x += g.adv;
        }
        if k == cands[ci] {
            ends.push(x);
            ci += 1;
        }
        k += 1;
    }
    if !modern {
        for n in (0..cands.len()).rev() {
            let k = cands[n];
            if gs[k].kind == Kind::Shy && ends[n] + hyphen_w(k) - u.right <= 1e-3 {
                return Some((k, hyphen_w(k)));
            }
        }
        let zone = if s.hyph_zone.is_nan() { HYPHEN_ZONE_DEFAULT } else { s.hyph_zone };
        if !u.line_start && u.right - u.x < zone - 1e-3 {
            return None;
        }
        for n in (0..cands.len()).rev() {
            let k = cands[n];
            if gs[k].kind == Kind::Shy {
                continue;
            }
            let mut nx = k + 1;
            while nx < u.e && (gs[nx].kind == Kind::Shy || (gs[nx].kind == Kind::Obj && gs[nx].adv == 0.0)) {
                nx += 1;
            }
            let end = (ends[n] + hyphen_w(k)).max(ends[n] + if nx < u.e { gs[nx].adv } else { 0.0 });
            if end - u.right <= 1e-3 {
                return Some((k, hyphen_w(k)));
            }
        }
        return None;
    }
    if !u.line_start {
        if justify {
            let slack = u.right - u.content_end;
            if u.rest_w > 0.0 && slack / u.rest_w <= HYPHEN_EXPAND15 {
                return None;
            }
        } else if u.right - u.content_end < HYPHEN_GAP15 {
            return None;
        }
    }
    let fits: Vec<f64> = cands.iter().enumerate().map(|(n, &k)| ends[n] + hyphen_w(k) - u.right).collect();
    for n in (0..cands.len()).rev() {
        let ov = fits[n];
        if ov <= 1e-3 {
            return Some((cands[n], hyphen_w(cands[n])));
        }
        if !justify || u.n_spaces == 0 {
            continue;
        }
        if ov > opts.shrink_ratio * u.spaces_w + 0.0175 {
            continue;
        }
        if n > 0 {
            let slack_prev = -fits[n - 1];
            if ov <= opts.expand_ratio * slack_prev + 0.02 {
                return Some((cands[n], hyphen_w(cands[n])));
            }
        } else {
            let slack = u.right - u.content_end;
            if u.line_start || u.rest_w <= 0.0 || ov / u.spaces_w <= (opts.expand_ratio * slack) / u.rest_w + 0.0075 * u.n_spaces as f64 / u.spaces_w {
                return Some((cands[n], hyphen_w(cands[n])));
            }
        }
    }
    None
}

#[derive(Clone, Copy)]
struct PendTab {
    idx: usize,
    pos: f64,
    tab: Tab,
    seg_start: f64,
    start_x: f64,
    decimal_seen: bool,
    decimal_x: f64,
}

pub struct LineBreaker<'a> {
    p: &'a Para,
    opts: &'a Opts,
    grid: Option<f64>,
    pub pos: usize,
    pub line_idx: usize,
    trailing_break: bool,
    hyphen_run: i32,
}

impl<'a> LineBreaker<'a> {
    pub fn new(p: &'a Para, opts: &'a Opts) -> Self {
        LineBreaker { p, opts, grid: p.grid, pos: 0, line_idx: 0, trailing_break: false, hyphen_run: 0 }
    }

    pub fn reset(&mut self, m: Mark) {
        self.pos = m.pos;
        self.line_idx = m.line_idx;
        self.trailing_break = m.trailing_break;
        self.hyphen_run = m.hyphen_run;
    }

    fn indent_first(&self) -> bool {
        let gs = &self.p.glyphs;
        self.line_idx == if gs.first().is_some_and(|g| g.kind == Kind::Cb) && gs.len() > 1 { 1 } else { 0 }
    }

    fn fit_right(&self, frame_left: f64, frame_right: f64, x_start: f64, first: bool) -> f64 {
        if !self.opts.round_word {
            return frame_right;
        }
        let p = self.p;
        let q = to_word_unit;
        x_start + q(frame_right + p.ind_right) - q(p.ind_right) - q(frame_left - p.ind_left) - q(p.ind_left) - if first { q(p.first_line) } else { 0.0 }
    }

    /// O glifo de um posicionado (o hífen automático é o glifo de origem como 'shy').
    #[inline]
    fn view(&self, q: &Placed) -> Glyph {
        if q.r >= 0 {
            self.p.glyphs[q.r as usize]
        } else if q.r == -1 {
            self.p.glyphs[q.src as usize].auto_hyphen()
        } else {
            self.p.marker.as_ref().unwrap().glyphs[(-2 - q.r) as usize]
        }
    }

    fn close_pending(pend: &mut Option<PendTab>, placed: &mut [Placed], x: &mut f64, content_end: &mut f64) {
        let Some(pt) = pend.take() else { return };
        let seg_w = *x - pt.seg_start;
        let mut shift = 0.0;
        let avail = pt.pos - pt.start_x;
        if pt.tab.val == TAB_RIGHT {
            shift = (avail - seg_w).max(0.0);
        } else if pt.tab.val == TAB_CENTER {
            shift = (avail - seg_w / 2.0).max(0.0);
        } else if pt.tab.val == TAB_DECIMAL {
            let dx = if pt.decimal_seen { pt.decimal_x - pt.seg_start } else { seg_w };
            shift = (avail - dx).max(0.0);
        }
        placed[pt.idx].w = shift;
        for q in &mut placed[pt.idx + 1..] {
            q.x += shift;
        }
        *x += shift;
        if *content_end > pt.seg_start {
            *content_end += shift;
        }
    }

    /// Próxima linha no quadro dado (que já desconta os recuos do parágrafo).
    pub fn next(&mut self, frame: Frame) -> Line {
        let p = self.p;
        let s = &p.settings;
        let gs = &p.glyphs[..];
        let ind_first = self.indent_first();
        let start = self.pos;
        let x_start = frame.left + if ind_first { p.first_line } else { 0.0 };
        let mut right = frame.right;
        let mut right_q = self.fit_right(frame.left, frame.right, x_start, ind_first);
        let mut placed: Vec<Placed> = Vec::new();
        let mut x = x_start;
        let mut marker_placed: Option<Vec<Placed>> = None;
        let default_tab = s.default_tab;
        let hanging_stop = if ind_first && p.first_line < 0.0 { Some(frame.left) } else { None };

        if ind_first {
            if let Some(mk) = &p.marker {
                let mut mx = x_start;
                if mk.jc == 2 {
                    mx -= mk.width;
                } else if mk.jc == 1 {
                    mx -= mk.width / 2.0;
                }
                let mut mp = Vec::with_capacity(mk.glyphs.len());
                for (k, g) in mk.glyphs.iter().enumerate() {
                    mp.push(Placed { r: -2 - k as i32, src: 0, x: mx, w: g.adv, leader: 0 });
                    mx += g.adv;
                }
                marker_placed = Some(mp);
                x = mx.max(x_start);
                if mk.suff == 1 {
                    let xe = x - 0.5 - 1e-3;
                    let mut ns = next_tab(xe, &p.tabs, default_tab, if frame.left > xe + 0.5 { Some(frame.left) } else { None });
                    if s.no_indent_tab && s.compat < 15 {
                        let plain = next_tab(xe, &p.tabs, default_tab, None);
                        ns = if frame.left > xe + 0.5 && frame.left < plain.pos { TabHit { pos: frame.left, tab: None } } else { plain };
                    }
                    x = ns.pos;
                } else if mk.suff == 2 {
                    x += mk.space_w;
                }
            }
        }

        let mut i = start;
        let mut last_break: isize = -1;
        let justify = (p.jc == JC_BOTH || p.jc == JC_DISTRIBUTE) && s.compat >= 15;
        let mut spaces_w = 0.0;
        let mut n_spaces: u32 = 0;
        let mut seg_word = false;
        let mut last_gap_w = 0.0;
        let mut last_gap_sp = 0.0;
        let (mut unit_x, mut unit_spaces_w, mut unit_gap_w, mut unit_gap_sp) = (x_start, 0.0, 0.0, 0.0);
        let mut fe_shrink_w = 0.0;
        let mut word_ok = false;
        let mut squeezed = false;
        let mut prev_was_gap = true;
        let mut width_at_break = 0.0;
        let mut content_end = x_start;
        let mut pend_tab: Option<PendTab> = None;
        let mut break_after: u8 = 0;
        let mut clear_after = false;
        let mut gap_tab: u8 = 0;
        let mut spanned = false;
        let mut past_margin = false;
        let mut forced = false;
        let (mut hyph_at, mut hyph_w, mut ended_hyphen): (isize, f64, bool) = (-1, 0.0, false);

        while i < gs.len() {
            let g = gs[i];
            if hyph_at >= 0 && i as isize > hyph_at {
                let ka = hyph_at as usize;
                let kg = gs[ka];
                if kg.kind == Kind::Shy {
                    placed.last_mut().unwrap().w = hyph_w;
                } else {
                    placed.push(Placed { r: -1, src: ka as u32, x, w: hyph_w, leader: 0 });
                }
                content_end = x + hyph_w;
                ended_hyphen = kg.kind != Kind::Shy;
                pend_tab = None;
                break;
            }
            if matches!(g.kind, Kind::Lb | Kind::Pb | Kind::Cb) {
                Self::close_pending(&mut pend_tab, &mut placed, &mut x, &mut content_end);
                placed.push(Placed { r: i as i32, src: 0, x, w: 0.0, leader: 0 });
                i += 1;
                forced = true;
                if g.kind == Kind::Pb {
                    break_after = 1;
                    let mut j = i;
                    while j < gs.len() && gs[j].kind == Kind::Obj && gs[j].adv == 0.0 && !gs[j].has(F_HR) {
                        j += 1;
                    }
                    if j > i && j >= gs.len() {
                        while i < j {
                            placed.push(Placed { r: i as i32, src: 0, x, w: 0.0, leader: 0 });
                            i += 1;
                        }
                    }
                } else if g.kind == Kind::Cb {
                    break_after = 2;
                } else if g.has(F_CLEAR) {
                    clear_after = true;
                }
                break;
            }
            if g.has(F_HR) {
                if placed.iter().any(|q| {
                    let v = self.view(q);
                    v.kind != Kind::Obj || v.adv > 0.0 || v.has(F_HR)
                }) {
                    break;
                }
                placed.push(Placed { r: i as i32, src: 0, x, w: 0.0, leader: 0 });
                i += 1;
                break;
            }
            if g.kind == Kind::Tab {
                Self::close_pending(&mut pend_tab, &mut placed, &mut x, &mut content_end);
                let mut pos: f64;
                let mut tab: Option<Tab>;
                if g.has(F_PTAB) {
                    let width = right - frame.left;
                    pos = match g.ptab_align {
                        1 => frame.left + width / 2.0,
                        2 => right,
                        _ => frame.left,
                    };
                    tab = Some(Tab { val: match g.ptab_align { 1 => TAB_CENTER, 2 => TAB_RIGHT, _ => TAB_LEFT }, pos, leader: g.ptab_leader });
                } else {
                    let h = next_tab(x, &p.tabs, default_tab, hanging_stop);
                    pos = h.pos;
                    tab = h.tab;
                }
                if frame.gap && !g.has(F_PTAB) && pos > right + 0.5 && more_tabs(gs, i) {
                    let nx = match frame.next {
                        Some((nl, _)) if !spanned => Some(next_tab(nl, &p.tabs, default_tab, hanging_stop)),
                        _ => None,
                    };
                    match (nx, frame.next) {
                        (Some(nx), Some((nl, nr))) if nx.pos <= nr + 0.5 => {
                            spanned = true;
                            x = nl;
                            right = nr;
                            right_q = self.fit_right(frame.left, right, x_start, ind_first);
                            pos = nx.pos;
                            tab = nx.tab;
                        }
                        _ => {
                            if i > start {
                                gap_tab = 1;
                                break;
                            }
                            gap_tab = 2;
                        }
                    }
                }
                let aligned = |t: &Option<Tab>| t.is_some_and(|t| matches!(t.val, TAB_RIGHT | TAB_CENTER | TAB_DECIMAL));
                if !g.has(F_PTAB) && aligned(&tab) && s.compat >= 15 && pos > right.max(right + p.ind_right) + 0.5 {
                    pos = right;
                    tab = tab.map(|t| Tab { val: TAB_RIGHT, ..t });
                }
                if !g.has(F_PTAB) && tab.is_some() && !aligned(&tab) && s.compat >= 15 && pos > right + 0.5 {
                    if i > start {
                        break;
                    }
                    pos = x.max(right);
                }
                let legacy_no_wrap = s.compat < 15 && p.tabs.iter().any(|t| t.val != TAB_BAR && t.val != TAB_CLEAR);
                if !g.has(F_PTAB) && tab.is_none() && !legacy_no_wrap && pos > right + 0.5 && i > start {
                    break;
                }
                if legacy_no_wrap && pos > right + 0.5 {
                    past_margin = true;
                }
                placed.push(Placed { r: i as i32, src: 0, x, w: 0.0, leader: tab.map_or(0, |t| t.leader) });
                if aligned(&tab) {
                    pend_tab = Some(PendTab { idx: placed.len() - 1, pos, tab: tab.unwrap(), seg_start: x, start_x: x, decimal_seen: false, decimal_x: 0.0 });
                } else {
                    placed.last_mut().unwrap().w = (pos - x).max(0.0);
                    x = x.max(pos);
                }
                last_break = if i > start { i as isize - 1 } else { i as isize };
                width_at_break = content_end - x_start;
                spaces_w = 0.0;
                n_spaces = 0;
                fe_shrink_w = 0.0;
                seg_word = false;
                last_gap_w = 0.0;
                last_gap_sp = 0.0;
                prev_was_gap = true;
                i += 1;
                continue;
            }
            if g.kind == Kind::Obj && g.adv == 0.0 {
                placed.push(Placed { r: i as i32, src: 0, x, w: 0.0, leader: 0 });
                i += 1;
                continue;
            }
            let is_gap = g.kind == Kind::Space;
            if !is_gap && g.kind != Kind::Shy && prev_was_gap {
                let mut e = i;
                let mut word_w = 0.0;
                while e < gs.len() {
                    let q = &gs[e];
                    if matches!(q.kind, Kind::Space | Kind::Tab | Kind::Lb | Kind::Pb | Kind::Cb) {
                        break;
                    }
                    if (q.kind == Kind::Obj && q.adv == 0.0) || q.kind == Kind::Shy {
                        e += 1;
                        continue;
                    }
                    word_w += q.adv;
                    if q.has(F_BR) && q.kind != Kind::Char {
                        break;
                    }
                    if q.has(F_BR) {
                        e += 1;
                        break;
                    }
                    e += 1;
                }
                let over = x + word_w - right_q;
                if i == start || !(gs[i - 1].kind == Kind::Space && !gs[i - 1].has(F_BR)) {
                    let gap = i > start && gs[i - 1].kind == Kind::Space;
                    unit_x = x;
                    unit_spaces_w = spaces_w;
                    unit_gap_w = if gap { last_gap_w } else { 0.0 };
                    unit_gap_sp = if gap { last_gap_sp } else { 0.0 };
                }
                word_ok = over <= FIT_EPS;
                let hang_w = if e > i && gs[e - 1].has(F_HANG) && (justify || s.compat < 15) { gs[e - 1].adv } else { 0.0 };
                if !word_ok && (hang_w > 0.0 || fe_shrink_w > 0.0) && i > start {
                    let para_end = gs[e..].iter().all(|q| q.kind == Kind::Pb || q.kind == Kind::Cb || (q.kind == Kind::Obj && q.adv == 0.0));
                    word_ok = over - hang_w <= (if hang_w > 0.0 && para_end { 0.0 } else { fe_shrink_w }) + FIT_EPS;
                }
                if !word_ok && justify && last_break >= start as isize && i > start {
                    let slack = x + word_w - unit_x + unit_gap_w - over;
                    let rest = unit_spaces_w - unit_gap_sp;
                    let ov = over - hang_w;
                    word_ok = n_spaces > 0
                        && ov <= self.opts.shrink_ratio * spaces_w + 0.0175
                        && (n_spaces <= 1 || rest <= 0.0 || ov / spaces_w <= (self.opts.expand_ratio * slack) / rest + 0.0075 * n_spaces as f64 / spaces_w);
                }
                if squeezed && i > start {
                    word_ok = false;
                } else if word_ok && over > FIT_EPS {
                    squeezed = true;
                } else if !word_ok && pend_tab.is_none() && !past_margin {
                    let u = HyphenUnit {
                        i, e, x, right: right_q, line_start: last_break < start as isize, content_end: x_start + width_at_break,
                        justify: p.jc == JC_BOTH || p.jc == JC_DISTRIBUTE, spaces_w, rest_w: unit_spaces_w - unit_gap_sp, n_spaces, hyphen_run: self.hyphen_run,
                    };
                    if let Some((k, w)) = choose_hyphenation(s, self.opts, gs, &u) {
                        hyph_at = k as isize;
                        hyph_w = w;
                        word_ok = true;
                    }
                }
            }
            let projected = if is_gap { x } else { x + g.adv };
            if !is_gap && g.kind != Kind::Shy && !word_ok && !past_margin && projected > right_q + FIT_EPS && i > start
                && pend_tab.is_some_and(|t| t.tab.val == TAB_RIGHT) && s.compat < 15
            {
                past_margin = true;
            }
            if !is_gap && g.kind != Kind::Shy && !word_ok && !past_margin && projected > right_q + FIT_EPS && i > start
                && placed.iter().any(|q| {
                    let v = self.view(q);
                    v.kind != Kind::Obj || v.adv > 0.0
                })
            {
                if last_break >= start as isize {
                    let keep = (last_break + 1) as usize;
                    while placed.last().is_some_and(|q| q.r != last_break as i32) {
                        placed.pop();
                    }
                    i = keep;
                    content_end = x_start + width_at_break;
                    pend_tab = None;
                    break;
                }
                if !g.has(F_GLUE) {
                    break;
                }
            }
            if g.kind == Kind::Shy {
                placed.push(Placed { r: i as i32, src: 0, x, w: 0.0, leader: 0 });
                i += 1;
                continue;
            }
            placed.push(Placed { r: i as i32, src: 0, x, w: g.adv, leader: 0 });
            if is_gap {
                if !prev_was_gap {
                    last_gap_w = 0.0;
                    last_gap_sp = 0.0;
                }
                last_gap_w += g.adv;
                if seg_word && !fixed_space(g.cp) && !g.has(F_FIXED) {
                    spaces_w += g.adv;
                    n_spaces += 1;
                    last_gap_sp += g.adv;
                    fe_shrink_w += g.fe_or0();
                }
            } else {
                seg_word = true;
            }
            prev_was_gap = is_gap || g.has(F_BR);
            if let Some(pt) = pend_tab.as_mut() {
                if pt.tab.val == TAB_DECIMAL && !pt.decimal_seen && g.has(F_DECIMAL) {
                    pt.decimal_seen = true;
                    pt.decimal_x = x;
                }
            }
            x += g.adv;
            if !is_gap {
                content_end = x;
            }
            if g.has(F_BR) {
                last_break = i as isize;
                width_at_break = (if is_gap { content_end } else { x }) - x_start;
            }
            i += 1;
        }
        Self::close_pending(&mut pend_tab, &mut placed, &mut x, &mut content_end);

        let mut width: f64 = 0.0;
        for q in &placed {
            let v = self.view(q);
            if v.kind == Kind::Obj && v.adv == 0.0 {
                continue;
            }
            if !matches!(v.kind, Kind::Space | Kind::Lb | Kind::Pb | Kind::Cb) {
                width = width.max(q.x + q.w - x_start);
            }
        }
        self.pos = gs.len().min(i);
        if self.pos == start && start < gs.len() {
            self.pos = start + 1;
        }
        self.hyphen_run = if ended_hyphen { self.hyphen_run + 1 } else { 0 };
        let mut lk_i = placed.len() as isize - 1;
        while lk_i > 0 && {
            let v = self.view(&placed[lk_i as usize]);
            v.kind == Kind::Obj && v.adv == 0.0 && !v.has(F_HR)
        } {
            lk_i -= 1;
        }
        let lk = if lk_i >= 0 && self.view(&placed[lk_i as usize]).kind == Kind::Pb { Some(Kind::Pb) } else { placed.last().map(|q| self.view(q).kind) };
        let split_pg = s.compat < 15 && s.split_pg;
        self.trailing_break = forced && self.pos >= gs.len() && (lk == Some(Kind::Lb) || lk == Some(Kind::Cb) || (lk == Some(Kind::Pb) && split_pg));
        let last = self.pos >= gs.len() && !self.trailing_break;
        self.line_idx += 1;

        // métricas verticais
        let (mut asc, mut desc, mut t_asc, mut t_desc): (f64, f64, f64, f64) = (0.0, 0.0, 0.0, 0.0);
        let (mut o_asc, mut o_desc): (f64, f64) = (0.0, 0.0);
        let is_content = |v: &Glyph| v.kind != Kind::Obj || v.adv > 0.0;
        let with_obj = placed.iter().any(|q| {
            let v = self.view(q);
            is_content(&v) && v.kind == Kind::Obj
        });
        for q in &placed {
            let v = self.view(q);
            if !is_content(&v) || v.kind == Kind::Pb || v.kind == Kind::Cb {
                continue;
            }
            if v.kind == Kind::Lb && with_obj {
                continue;
            }
            if v.has(F_CHSPACE) || v.kind == Kind::Tab {
                continue;
            }
            asc = asc.max(v.asc);
            desc = desc.max(v.desc);
            if v.kind != Kind::Obj {
                t_asc = t_asc.max(v.n_asc_or_asc());
                t_desc = t_desc.max(v.n_desc_or_desc());
            } else {
                o_asc = o_asc.max(v.fa * v.size_pt * 20.0);
                o_desc = o_desc.max(v.fd * v.size_pt * 20.0);
                if v.has(F_UL) {
                    desc = desc.max(v.fd * v.size_pt * 20.0);
                }
            }
        }
        if t_asc + t_desc == 0.0 && o_asc > 0.0 {
            asc = asc.max(o_asc + o_desc - desc);
        }
        for q in &placed {
            let v = self.view(q);
            if !v.has(F_HR) {
                continue;
            }
            let fa = v.fa * v.size_pt * 20.0;
            let fd = v.fd * v.size_pt * 20.0;
            t_asc = t_asc.max(fa);
            t_desc = t_desc.max(fd);
            desc = desc.max(fd);
            asc = asc.max(fa).max(v.hr + 100.0 - desc);
        }
        if !last && asc == 0.0 && desc == 0.0 {
            let mut any_tab = false;
            let mut all_tabs = true;
            for q in &placed {
                let v = self.view(q);
                if !is_content(&v) {
                    continue;
                }
                if v.kind == Kind::Tab {
                    any_tab = true;
                } else if !v.has(F_CHSPACE) {
                    all_tabs = false;
                }
            }
            if any_tab && all_tabs {
                for q in &placed {
                    let v = self.view(q);
                    if !is_content(&v) || v.kind != Kind::Tab {
                        continue;
                    }
                    asc = asc.max(v.asc);
                    desc = desc.max(v.desc);
                    t_asc = t_asc.max(v.n_asc_or_asc());
                    t_desc = t_desc.max(v.n_desc_or_desc());
                }
            }
        }
        let empty_line = asc == 0.0 && desc == 0.0;
        if empty_line {
            let va = p.mark_va;
            let sz = (if va == 1 || va == 2 { p.mark_sz.unwrap_or(20.0) / 2.0 } else { p.mark_size }) * 20.0;
            let sh = if va != 0 { 0.0 } else { p.mark_shift };
            let (ea, ed) = p.shadow;
            asc = (p.mark_asc * sz + ea + sh).max(0.0);
            desc = (p.mark_desc * sz + ed - sh).max(0.0);
        }
        if t_asc + t_desc == 0.0 {
            t_asc = if o_asc != 0.0 { o_asc } else if empty_line { asc } else { p.mark_asc * p.mark_size * 20.0 };
            t_desc = if o_asc != 0.0 { o_desc } else if empty_line { desc } else { p.mark_desc * p.mark_size * 20.0 };
        }
        if let Some(mp) = &marker_placed {
            for q in mp {
                asc = asc.max(self.view(q).asc);
            }
        }
        let (height, baseline) = line_box(asc, desc, p, if t_asc + t_desc > 0.0 { Some(t_asc + t_desc) } else { None }, self.grid);

        // alinhamento/justificação
        let avail = right - x_start;
        let mut dx = 0.0;
        let jc = p.jc;
        let mut last_c: Option<usize> = None;
        for (k, q) in placed.iter().enumerate() {
            if !matches!(self.view(q).kind, Kind::Space | Kind::Lb | Kind::Pb | Kind::Cb) {
                last_c = Some(k);
            }
        }
        let align_w = match last_c {
            Some(k) if self.view(&placed[k]).has(F_HANG) && width > avail + FIT_EPS => width - placed[k].w,
            _ => width,
        };
        let justified = (jc == JC_BOTH || jc == JC_DISTRIBUTE) && (!last || jc == JC_DISTRIBUTE) && break_after == 0;
        if !justified && align_w > avail + FIT_EPS {
            let lc_x = last_c.map_or(0.0, |k| placed[k].x);
            let sh: Vec<bool> = placed.iter().map(|q| self.view(q).fe_or0() != 0.0 && q.x < lc_x).collect();
            let mut total = 0.0;
            for (k, q) in placed.iter().enumerate() {
                if sh[k] {
                    total += self.view(q).fe_or0();
                }
            }
            if total > 0.0 {
                let f = ((align_w - avail) / total).min(1.0);
                let mut acc = 0.0;
                for k in 0..placed.len() {
                    let fe = self.view(&placed[k]).fe_or0();
                    placed[k].x -= acc;
                    if fe != 0.0 && sh[k] {
                        placed[k].w -= fe * f;
                        acc += fe * f;
                    }
                }
            }
        }
        if jc == JC_CENTER {
            dx = (avail - align_w) / 2.0;
        } else if jc == JC_RIGHT {
            dx = avail - align_w;
        } else if justified {
            let mut last_tab: isize = -1;
            let mut last_content: isize = -1;
            for (k, q) in placed.iter().enumerate() {
                let v = self.view(q);
                if v.kind == Kind::Tab {
                    last_tab = k as isize;
                }
                if v.kind != Kind::Space && v.kind != Kind::Lb {
                    last_content = k as isize;
                }
            }
            let mut first_content = placed.len();
            for k in (last_tab + 1) as usize..placed.len() {
                if self.view(&placed[k]).kind != Kind::Space {
                    first_content = k;
                    break;
                }
            }
            let spaces: Vec<bool> = placed
                .iter()
                .enumerate()
                .map(|(k, q)| {
                    let v = self.view(q);
                    k > first_content && (k as isize) < last_content && v.kind == Kind::Space && !fixed_space(v.cp) && !v.has(F_FIXED)
                })
                .collect();
            let n = spaces.iter().filter(|b| **b).count();
            let extra = avail - align_w;
            if n > 0 && (extra > 0.0 || extra < 0.0) {
                let each = extra / n as f64;
                let mut acc = 0.0;
                for k in 0..placed.len() {
                    placed[k].x += acc;
                    if spaces[k] {
                        placed[k].w += each;
                        acc += each;
                    }
                }
            }
        }
        if dx > 0.0 || dx < 0.0 {
            for q in &mut placed {
                q.x += dx;
            }
            if jc != JC_LEFT {
                if let Some(mp) = marker_placed.as_mut() {
                    for q in mp {
                        q.x += dx;
                    }
                }
            }
        }
        let hr = placed.iter().any(|q| self.view(q).has(F_HR));
        Line {
            start, end: self.pos, placed, marker: marker_placed, width, x_start: x_start + dx.max(0.0), asc, desc, height, baseline,
            break_after, clear: clear_after, gap_tab, spanned, hr, last,
        }
    }
}

/// Repete a sequência de chamadas da captura (new / next / reset); devolve as linhas.
pub fn replay(c: &crate::linedata::Corpus) -> Vec<Line> {
    let mut brs: Vec<Option<LineBreaker>> = (0..c.paras.len()).map(|_| None).collect();
    let mut out = Vec::with_capacity(c.expected.len());
    for e in &c.events {
        match *e {
            Event::New(id) => brs[id] = Some(LineBreaker::new(&c.paras[id], &c.opts)),
            Event::Next(id, f) => out.push(brs[id].as_mut().unwrap().next(f)),
            Event::Reset(id, m) => brs[id].as_mut().unwrap().reset(m),
        }
    }
    out
}
