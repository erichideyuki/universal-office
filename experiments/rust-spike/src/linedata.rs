//! Modelo de entrada/saída da quebra de linha (o que a captura do TS gravou em lines.bin) e o
//! leitor do formato binário de pack.mjs.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Kind {
    Char = 0,
    Space,
    Nbsp,
    Tab,
    Lb,
    Pb,
    Cb,
    Obj,
    Shy,
    Box,
}

pub const F_BR: u16 = 1;
pub const F_GLUE: u16 = 2;
pub const F_CLEAR: u16 = 4;
pub const F_HR: u16 = 8;
pub const F_PTAB: u16 = 16;
pub const F_FIXED: u16 = 32;
pub const F_HANG: u16 = 64;
pub const F_HYPH: u16 = 128;
pub const F_UL: u16 = 256;
pub const F_LETTER: u16 = 512;
pub const F_CHSPACE: u16 = 1024;
pub const F_DECIMAL: u16 = 2048;
pub const F_NASC: u16 = 4096;
pub const F_NDESC: u16 = 8192;
pub const F_FE: u16 = 16384;

/// Glifo preparado (o Glyph do para.ts, só com o que a quebra lê). `hy_w`, `fa`, `fd`: largura
/// do hífen e métricas da face, pré-calculadas na captura.
#[derive(Clone, Copy, Debug)]
pub struct Glyph {
    pub kind: Kind,
    pub flags: u16,
    pub cp: u32,
    pub adv: f64,
    pub asc: f64,
    pub desc: f64,
    pub n_asc: f64,
    pub n_desc: f64,
    pub hr: f64,
    pub fe: f64,
    pub hy_w: f64,
    pub fa: f64,
    pub fd: f64,
    pub size_pt: f64,
    pub ptab_align: u8,
    pub ptab_leader: u8,
}

impl Glyph {
    #[inline]
    pub fn has(&self, f: u16) -> bool {
        self.flags & f != 0
    }
    #[inline]
    pub fn n_asc_or_asc(&self) -> f64 {
        if self.has(F_NASC) { self.n_asc } else { self.asc }
    }
    #[inline]
    pub fn n_desc_or_desc(&self) -> f64 {
        if self.has(F_NDESC) { self.n_desc } else { self.desc }
    }
    /// feShrink ?? 0
    #[inline]
    pub fn fe_or0(&self) -> f64 {
        if self.has(F_FE) { self.fe } else { 0.0 }
    }
    /// o hífen automático que a quebra acrescenta: { ...kg, kind: 'shy', ch: '', cp: 0xad, adv: 0, hyph: false }
    pub fn auto_hyphen(&self) -> Glyph {
        Glyph { kind: Kind::Shy, flags: self.flags & !(F_HYPH | F_LETTER | F_CHSPACE | F_DECIMAL), cp: 0xad, adv: 0.0, ..*self }
    }
}

pub const TAB_LEFT: u8 = 0;
pub const TAB_CENTER: u8 = 1;
pub const TAB_RIGHT: u8 = 2;
pub const TAB_DECIMAL: u8 = 3;
pub const TAB_BAR: u8 = 4;
pub const TAB_CLEAR: u8 = 5;

#[derive(Clone, Copy, Debug)]
pub struct Tab {
    pub val: u8,
    pub pos: f64,
    pub leader: u8,
}

pub const JC_LEFT: u8 = 0;
pub const JC_CENTER: u8 = 1;
pub const JC_RIGHT: u8 = 2;
pub const JC_BOTH: u8 = 3;
pub const JC_DISTRIBUTE: u8 = 4;
pub const RULE_AUTO: u8 = 0;
pub const RULE_EXACT: u8 = 1;
pub const RULE_ATLEAST: u8 = 2;

#[derive(Clone, Debug)]
pub struct Settings {
    pub default_tab: f64,
    pub compat: i32,
    pub no_indent_tab: bool,
    pub split_pg: bool,
    pub hyph_limit: i32,
    /// NaN = ausente
    pub hyph_zone: f64,
}

#[derive(Clone, Debug)]
pub struct Marker {
    /// 0 left, 1 center, 2 right
    pub jc: u8,
    pub width: f64,
    /// 0 nada, 1 tab, 2 espaço
    pub suff: u8,
    pub space_w: f64,
    pub glyphs: Vec<Glyph>,
}

#[derive(Clone, Debug)]
pub struct Para {
    pub settings: Settings,
    pub grid: Option<f64>,
    pub first_line: f64,
    pub ind_left: f64,
    pub ind_right: f64,
    pub jc: u8,
    pub line_rule: u8,
    pub line: f64,
    pub mark_asc: f64,
    pub mark_desc: f64,
    pub mark_size: f64,
    pub mark_shift: f64,
    /// 0 nada, 1 sobrescrito, 2 subscrito
    pub mark_va: u8,
    pub mark_sz: Option<f64>,
    pub shadow: (f64, f64),
    pub tabs: Vec<Tab>,
    pub glyphs: Vec<Glyph>,
    pub marker: Option<Marker>,
}

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub left: f64,
    pub right: f64,
    pub gap: bool,
    pub next: Option<(f64, f64)>,
}

#[derive(Clone, Copy, Debug)]
pub struct Mark {
    pub pos: usize,
    pub line_idx: usize,
    pub trailing_break: bool,
    pub hyphen_run: i32,
}

#[derive(Clone, Copy, Debug)]
pub enum Event {
    New(usize),
    Next(usize, Frame),
    Reset(usize, Mark),
}

/// Glifo posicionado. `r`: índice no parágrafo; -1 = hífen automático (do glifo `src`);
/// -2-k = k-ésimo glifo do marcador.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub r: i32,
    pub src: u32,
    pub x: f64,
    pub w: f64,
    pub leader: u8,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub start: usize,
    pub end: usize,
    pub width: f64,
    pub x_start: f64,
    pub asc: f64,
    pub desc: f64,
    pub height: f64,
    pub baseline: f64,
    pub last: bool,
    /// 0 nada, 1 página, 2 coluna
    pub break_after: u8,
    pub clear: bool,
    /// 0 nada, 1 break, 2 start
    pub gap_tab: u8,
    pub spanned: bool,
    pub hr: bool,
    pub placed: Vec<Placed>,
    pub marker: Option<Vec<Placed>>,
}

pub struct Opts {
    pub shrink_ratio: f64,
    pub expand_ratio: f64,
    /// advanceRound 'word' e advanceEm ≠ 'device' (o "cabe" na unidade interna do Word)
    pub round_word: bool,
}

pub struct Corpus {
    pub opts: Opts,
    pub paras: Vec<Para>,
    pub events: Vec<Event>,
    pub expected: Vec<Line>,
}

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl Rd<'_> {
    fn u8(&mut self) -> u8 {
        let v = self.b[self.p];
        self.p += 1;
        v
    }
    fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.b[self.p..self.p + 4].try_into().unwrap());
        self.p += 4;
        v
    }
    fn i32(&mut self) -> i32 {
        self.u32() as i32
    }
    fn f64(&mut self) -> f64 {
        let v = f64::from_le_bytes(self.b[self.p..self.p + 8].try_into().unwrap());
        self.p += 8;
        v
    }
    fn opt(&mut self) -> Option<f64> {
        let v = self.f64();
        if v.is_nan() { None } else { Some(v) }
    }
    fn glyph(&mut self) -> Glyph {
        let kind = match self.u8() {
            0 => Kind::Char,
            1 => Kind::Space,
            2 => Kind::Nbsp,
            3 => Kind::Tab,
            4 => Kind::Lb,
            5 => Kind::Pb,
            6 => Kind::Cb,
            7 => Kind::Obj,
            8 => Kind::Shy,
            _ => Kind::Box,
        };
        let flags = self.u8() as u16 | (self.u8() as u16) << 8;
        let cp = self.u32();
        let mut f = [0.0; 12];
        for v in f.iter_mut() {
            *v = self.f64();
        }
        let ptab_align = self.u8();
        let ptab_leader = self.u8();
        Glyph {
            kind, flags, cp, adv: f[0], asc: f[1], desc: f[2], n_asc: f[3], n_desc: f[4], hr: f[5], fe: f[6], hy_w: f[7], fa: f[8], fd: f[9], size_pt: f[10],
            ptab_align, ptab_leader,
        }
    }
    fn glyphs(&mut self) -> Vec<Glyph> {
        let n = self.u32() as usize;
        (0..n).map(|_| self.glyph()).collect()
    }
    fn placed(&mut self) -> Vec<Placed> {
        let n = self.u32() as usize;
        (0..n).map(|_| Placed { r: self.i32(), src: 0, x: self.f64(), w: self.f64(), leader: self.u8() }).collect()
    }
}

pub fn read_corpus(b: &[u8]) -> Corpus {
    let mut r = Rd { b, p: 0 };
    assert_eq!(r.u32(), 0x31534c55, "lines.bin inválido");
    let opts = Opts { shrink_ratio: r.f64(), expand_ratio: r.f64(), round_word: true };
    let np = r.u32() as usize;
    let mut paras = Vec::with_capacity(np);
    for _ in 0..np {
        let settings = Settings { default_tab: r.f64(), compat: r.i32(), no_indent_tab: r.u8() != 0, split_pg: r.u8() != 0, hyph_limit: r.i32(), hyph_zone: r.f64() };
        let grid = r.opt();
        let (first_line, ind_left, ind_right) = (r.f64(), r.f64(), r.f64());
        let (jc, line_rule, line) = (r.u8(), r.u8(), r.f64());
        let (mark_asc, mark_desc) = (r.f64(), r.f64());
        let (mark_size, mark_shift, mark_va, mark_sz) = (r.f64(), r.f64(), r.u8(), r.opt());
        let shadow = (r.f64(), r.f64());
        let nt = r.u32() as usize;
        let tabs = (0..nt).map(|_| Tab { val: r.u8(), pos: r.f64(), leader: r.u8() }).collect();
        let glyphs = r.glyphs();
        let marker = if r.u8() != 0 {
            let (jc, width, suff, space_w) = (r.u8(), r.f64(), r.u8(), r.f64());
            Some(Marker { jc, width, suff, space_w, glyphs: r.glyphs() })
        } else {
            None
        };
        paras.push(Para {
            settings, grid, first_line, ind_left, ind_right, jc, line_rule, line, mark_asc, mark_desc, mark_size, mark_shift, mark_va, mark_sz, shadow, tabs,
            glyphs, marker,
        });
    }
    let ne = r.u32() as usize;
    let mut events = Vec::with_capacity(ne);
    for _ in 0..ne {
        let op = r.u8();
        let id = r.u32() as usize;
        events.push(match op {
            0 => Event::New(id),
            1 => {
                let (left, right, gap, has_next, nl, nr) = (r.f64(), r.f64(), r.u8() != 0, r.u8() != 0, r.f64(), r.f64());
                Event::Next(id, Frame { left, right, gap, next: if has_next { Some((nl, nr)) } else { None } })
            }
            _ => Event::Reset(id, Mark { pos: r.u32() as usize, line_idx: r.u32() as usize, trailing_break: r.u8() != 0, hyphen_run: r.u32() as i32 }),
        });
    }
    let nl = r.u32() as usize;
    let mut expected = Vec::with_capacity(nl);
    for _ in 0..nl {
        let (start, end) = (r.u32() as usize, r.u32() as usize);
        let (width, x_start, asc, desc, height, baseline) = (r.f64(), r.f64(), r.f64(), r.f64(), r.f64(), r.f64());
        let (last, break_after, clear, gap_tab, spanned, hr) = (r.u8() != 0, r.u8(), r.u8() != 0, r.u8(), r.u8() != 0, r.u8() != 0);
        let placed = r.placed();
        let marker = if r.u8() != 0 { Some(r.placed()) } else { None };
        expected.push(Line { start, end, width, x_start, asc, desc, height, baseline, last, break_after, clear, gap_tab, spanned, hr, placed, marker });
    }
    Corpus { opts, paras, events, expected }
}

/// Linhas diferentes da captura do TS (igualdade exata dos números).
pub fn mismatches(got: &[Line], expected: &[Line]) -> usize {
    let same_p = |a: &[Placed], b: &[Placed]| a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.r == q.r && p.x == q.x && p.w == q.w && p.leader == q.leader);
    let mut bad = got.len().abs_diff(expected.len());
    for (a, b) in got.iter().zip(expected) {
        let same = a.start == b.start && a.end == b.end && a.width == b.width && a.x_start == b.x_start && a.asc == b.asc && a.desc == b.desc
            && a.height == b.height && a.baseline == b.baseline && a.last == b.last && a.break_after == b.break_after && a.clear == b.clear
            && a.gap_tab == b.gap_tab && a.spanned == b.spanned && same_p(&a.placed, &b.placed)
            && match (&a.marker, &b.marker) {
                (None, None) => true,
                (Some(x), Some(y)) => same_p(x, y),
                _ => false,
            };
        if !same {
            bad += 1;
        }
    }
    bad
}
