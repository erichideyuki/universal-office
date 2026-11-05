//! Character (CHP), paragraph (PAP), table (TAP) and section (SEP) properties and applying sprms
//! to them ([MS-DOC] 2.6, `props.ts`). Every application marks the field as "touched": the OOXML
//! writer emits only what was touched (direct formatting / the style's delta), with the final
//! value already resolved (the toggles 0x80/0x81 against the style's value).
//!
//! `Chp` and `Pap` are structs of `Option` fields (an absent key is `None`; an explicit
//! `undefined` is unobservable in the TS: every reader tests `!== undefined`), generated with
//! their key enums by one macro: `has`, `copy_key` (`chp[k] = style[k]`), `clear_key` (`delete
//! chp[k]`) and `field` (the digest of the parity facet). The touched sets are insertion-ordered
//! ([`KeySet`], a JS `Set`). The TS object key order of `Chp`/`Pap` is not observable (only
//! `Object.keys` in `setAll` reads it) and is not kept.

use super::bytes::ByteReader;
use super::sprm::{Prl, prls};

// ───────────────────────────── helpers ─────────────────────────────

/// A colour: 0xRRGGBB or [`AUTO`].
pub const AUTO: i32 = -1;

/// A border (`Brc`). `from_new` is not a TS field: it records the key order of the object the
/// TS built (`brcNew` writes `color` first, `brc80` writes `width` first), which
/// `JSON.stringify` shows and `sprmCMajority` compares (declaration 4 of F2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Brc {
    /// 0xRRGGBB or [`AUTO`]
    pub color: i32,
    /// width in 1/8 pt
    pub width: u8,
    /// BrcType ([MS-DOC] 2.9.17): 0 none, 0xFF nil
    pub brc_type: u8,
    /// distance to the text, in points
    pub space: u8,
    pub shadow: bool,
    pub frame: bool,
    pub from_new: bool,
}

/// A shading (`Shd`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shd {
    pub fore: i32,
    pub back: i32,
    /// Ipat ([MS-DOC] 2.9.127): 0xFFFF = nil
    pub pat: u16,
}

/// `ICO_RGB`: Ico ([MS-DOC] 2.9.119) to colour.
pub const ICO_RGB: [i32; 17] = [
    AUTO, 0x000000, 0x0000ff, 0x00ffff, 0x00ff00, 0xff00ff, 0xff0000, 0xffff00, 0xffffff, 0x000080, 0x008080, 0x008000, 0x800080, 0x800000,
    0x808000, 0x808080, 0xc0c0c0,
];

/// `icoColor`.
#[must_use]
pub fn ico_color(ico: u8) -> i32 {
    ICO_RGB.get(usize::from(ico)).copied().unwrap_or(AUTO)
}

/// `colorRef`: COLORREF (r, g, b, fAuto) to colour.
#[must_use]
pub fn color_ref(v: u32) -> i32 {
    if v >> 24 == 0xff {
        return AUTO;
    }
    let (r, g, b) = (v & 0xff, (v >> 8) & 0xff, (v >> 16) & 0xff);
    // at most 24 bits
    i32::try_from((r << 16) | (g << 8) | b).unwrap_or(AUTO)
}

/// `brc80`: Brc80 (4 bytes); 0xFFFFFFFF is nil (`Brc80MayBeNil`).
#[must_use]
pub fn brc80(r: ByteReader<'_>, off: u64) -> Option<Brc> {
    if r.u32(off) == 0xffff_ffff {
        return None;
    }
    let flags = r.u8(off + 3);
    Some(Brc {
        width: r.u8(off),
        brc_type: r.u8(off + 1),
        color: ico_color(r.u8(off + 2)),
        space: flags & 0x1f,
        shadow: flags & 0x20 != 0,
        frame: flags & 0x40 != 0,
        from_new: false,
    })
}

/// `brcNew`: Brc (8 bytes): cv, dptLineWidth, brcType, dptSpace/flags.
#[must_use]
pub fn brc_new(r: ByteReader<'_>, off: u64) -> Option<Brc> {
    if r.u32(off) == 0xffff_ffff && r.u32(off + 4) == 0xffff_ffff {
        return None;
    }
    let flags = r.u16(off + 6);
    Some(Brc {
        color: color_ref(r.u32(off)),
        width: r.u8(off + 4),
        brc_type: r.u8(off + 5),
        space: (flags & 0x1f) as u8,
        shadow: flags & 0x20 != 0,
        frame: flags & 0x40 != 0,
        from_new: true,
    })
}

/// `shd80`: SHD80 (u16): icoFore 5 bits, icoBack 5 bits, ipat 6 bits.
#[must_use]
pub fn shd80(v: u16) -> Shd {
    let pat = (v >> 10) & 0x3f;
    Shd { fore: ico_color((v & 0x1f) as u8), back: ico_color(((v >> 5) & 0x1f) as u8), pat: if pat == 0x3f { 0xffff } else { pat } }
}

/// `shdNew`: SHD (10 bytes): cvFore, cvBack, ipat.
#[must_use]
pub fn shd_new(r: ByteReader<'_>, off: u64) -> Shd {
    Shd { fore: color_ref(r.u32(off)), back: color_ref(r.u32(off + 4)), pat: r.u16(off + 8) }
}

/// An insertion-ordered set of keys (a JS `Set`): `add` of a present key keeps its place, `delete`
/// then `add` moves it to the end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySet<K> {
    order: Vec<K>,
}

impl<K> Default for KeySet<K> {
    fn default() -> Self {
        KeySet { order: Vec::new() }
    }
}

impl<K: Copy + PartialEq> KeySet<K> {
    #[must_use]
    pub fn new() -> KeySet<K> {
        KeySet::default()
    }

    pub fn add(&mut self, k: K) {
        if !self.order.contains(&k) {
            self.order.push(k);
        }
    }

    #[must_use]
    pub fn has(&self, k: K) -> bool {
        self.order.contains(&k)
    }

    pub fn delete(&mut self, k: K) {
        if let Some(i) = self.order.iter().position(|&x| x == k) {
            self.order.remove(i);
        }
    }

    pub fn clear(&mut self) {
        self.order.clear();
    }

    /// The keys in insertion order.
    #[must_use]
    pub fn keys(&self) -> &[K] {
        &self.order
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

// ───────────────────────────── the property structs ─────────────────────────────

/// What `Chp::field` and `Pap::field` return: a field value as the digest of the parity facet
/// sees it.
#[derive(Debug, Clone, Copy)]
pub enum FieldRef<'a> {
    Bool(bool),
    Num(f64),
    Brc(&'a Brc),
    Shd(&'a Shd),
    Symbol(&'a Symbol),
    Line(&'a LineSpacing),
    Tabs(&'a [TabStop]),
    Shorts(&'a [i16]),
    Bytes(&'a [u8]),
}

/// A field type of the property structs.
pub trait Field {
    fn field_ref(&self) -> FieldRef<'_>;
}

macro_rules! num_field {
    ($($t:ty),*) => {$(impl Field for $t { fn field_ref(&self) -> FieldRef<'_> { FieldRef::Num(f64::from(*self)) } })*};
}
num_field!(u8, u16, i16, i32, u32);

impl Field for i64 {
    fn field_ref(&self) -> FieldRef<'_> {
        // the values are far below 2^53 (accumulated 16/32-bit operands)
        FieldRef::Num(*self as f64)
    }
}

impl Field for bool {
    fn field_ref(&self) -> FieldRef<'_> {
        FieldRef::Bool(*self)
    }
}

impl Field for Brc {
    fn field_ref(&self) -> FieldRef<'_> {
        FieldRef::Brc(self)
    }
}

impl Field for Shd {
    fn field_ref(&self) -> FieldRef<'_> {
        FieldRef::Shd(self)
    }
}

/// `symbol`: sprmCSymbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Symbol {
    pub ftc: u16,
    pub xchar: u16,
}

impl Field for Symbol {
    fn field_ref(&self) -> FieldRef<'_> {
        FieldRef::Symbol(self)
    }
}

/// `lineSpacing`: sprmPDyaLine (`lspd`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineSpacing {
    pub line: i16,
    pub mult: bool,
}

impl Field for LineSpacing {
    fn field_ref(&self) -> FieldRef<'_> {
        FieldRef::Line(self)
    }
}

/// A tab stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabStop {
    pub pos: i16,
    /// 0 left, 1 centre, 2 right, 3 decimal, 4 bar, 6 list
    pub jc: u8,
    /// 0 none, 1 dots, 2 hyphens, 3 line, 4 heavy, 5 middle dot
    pub tlc: u8,
}

impl Field for Vec<TabStop> {
    fn field_ref(&self) -> FieldRef<'_> {
        FieldRef::Tabs(self)
    }
}

impl Field for Vec<i16> {
    fn field_ref(&self) -> FieldRef<'_> {
        FieldRef::Shorts(self)
    }
}

impl Field for Vec<u8> {
    fn field_ref(&self) -> FieldRef<'_> {
        FieldRef::Bytes(self)
    }
}

macro_rules! props {
    (
        $(#[$meta:meta])*
        struct $name:ident, key $key:ident,
        req { $($rf:ident : $rt:ty,)* }
        opt { $($f:ident, $v:ident, $js:literal, $t:ty;)* }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Default, PartialEq)]
        pub struct $name {
            $(pub $rf: $rt,)*
            $(pub $f: Option<$t>,)*
        }

        /// The keys of the optional fields, in the order of the TS interface.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $key {
            $($v,)*
        }

        impl $key {
            /// Every key, in the order of the TS interface.
            pub const ALL: &'static [$key] = &[$($key::$v,)*];

            /// The TS key (`keyof`).
            #[must_use]
            pub fn name(self) -> &'static str {
                match self { $($key::$v => $js,)* }
            }
        }

        impl $name {
            /// `obj[k] !== undefined`.
            #[must_use]
            pub fn has(&self, k: $key) -> bool {
                match k { $($key::$v => self.$f.is_some(),)* }
            }

            /// `obj[k] = from[k]` (an absent value is `undefined`).
            pub fn copy_key(&mut self, k: $key, from: &$name) {
                match k { $($key::$v => self.$f = from.$f.clone(),)* }
            }

            /// `delete obj[k]`.
            pub fn clear_key(&mut self, k: $key) {
                match k { $($key::$v => self.$f = None,)* }
            }

            /// `JSON.stringify(a[k]) === JSON.stringify(b[k])` for the values the TS stores.
            #[must_use]
            pub fn same_json(&self, k: $key, other: &$name) -> bool {
                match k { $($key::$v => self.$f == other.$f,)* }
            }

            /// The value of the field, for the digest of the parity facet.
            #[must_use]
            pub fn field(&self, k: $key) -> Option<FieldRef<'_>> {
                match k { $($key::$v => self.$f.as_ref().map(Field::field_ref),)* }
            }
        }
    };
}

props! {
    /// The character properties (`Chp`).
    struct Chp, key ChpKey,
    req {}
    opt {
        bold, Bold, "bold", bool;
        italic, Italic, "italic", bool;
        strike, Strike, "strike", bool;
        dstrike, Dstrike, "dstrike", bool;
        outline, Outline, "outline", bool;
        shadow, Shadow, "shadow", bool;
        emboss, Emboss, "emboss", bool;
        imprint, Imprint, "imprint", bool;
        small_caps, SmallCaps, "smallCaps", bool;
        caps, Caps, "caps", bool;
        vanish, Vanish, "vanish", bool;
        spec_vanish, SpecVanish, "specVanish", bool;
        web_hidden, WebHidden, "webHidden", bool;
        bold_bi, BoldBi, "boldBi", bool;
        italic_bi, ItalicBi, "italicBi", bool;
        rtl, Rtl, "rtl", bool;
        cs, Cs, "cs", bool;
        no_proof, NoProof, "noProof", bool;
        kul, Kul, "kul", u8;
        cv_ul, CvUl, "cvUl", i32;
        hps, Hps, "hps", i32;
        hps_bi, HpsBi, "hpsBi", u16;
        hps_pos, HpsPos, "hpsPos", i16;
        hps_kern, HpsKern, "hpsKern", u16;
        ico, Ico, "ico", u8;
        cv, Cv, "cv", i32;
        highlight, Highlight, "highlight", u8;
        iss, Iss, "iss", u8;
        dxa_space, DxaSpace, "dxaSpace", i16;
        char_scale, CharScale, "charScale", u16;
        kcd, Kcd, "kcd", u8;
        ftc_ascii, FtcAscii, "ftcAscii", u16;
        ftc_fe, FtcFE, "ftcFE", u16;
        ftc_other, FtcOther, "ftcOther", u16;
        ftc_bi, FtcBi, "ftcBi", u16;
        lid, Lid, "lid", u16;
        lid_fe, LidFE, "lidFE", u16;
        lid_bi, LidBi, "lidBi", u16;
        istd, Istd, "istd", u16;
        f_spec, FSpec, "fSpec", bool;
        f_data, FData, "fData", bool;
        f_ole2, FOle2, "fOle2", bool;
        f_obj, FObj, "fObj", bool;
        pic_location, PicLocation, "picLocation", i32;
        symbol, Symbol, "symbol", Symbol;
        shd, Shd, "shd", Shd;
        brc, Brc, "brc", Brc;
        rmark_del, RmarkDel, "rmarkDel", bool;
        rmark_ins, RmarkIns, "rmarkIns", bool;
        ibst_rmark, IbstRMark, "ibstRMark", u16;
        dttm_rmark, DttmRMark, "dttmRMark", u32;
        ibst_rmark_del, IbstRMarkDel, "ibstRMarkDel", u16;
        dttm_rmark_del, DttmRMarkDel, "dttmRMarkDel", u32;
        fld_vanish, FldVanish, "fldVanish", bool;
        idct_hint, IdctHint, "idctHint", u8;
        sfx_text, SfxText, "sfxText", u8;
    }
}

props! {
    /// The paragraph properties (`Pap`); `istd` is always present.
    struct Pap, key PapKey,
    req { istd: u16, }
    opt {
        jc, Jc, "jc", u8;
        dxa_left, DxaLeft, "dxaLeft", i64;
        dxa_right, DxaRight, "dxaRight", i16;
        dxa_left1, DxaLeft1, "dxaLeft1", i16;
        dxc_left, DxcLeft, "dxcLeft", i16;
        dxc_right, DxcRight, "dxcRight", i16;
        dxc_left1, DxcLeft1, "dxcLeft1", i16;
        dya_before, DyaBefore, "dyaBefore", u16;
        dya_after, DyaAfter, "dyaAfter", u16;
        before_auto, BeforeAuto, "beforeAuto", bool;
        after_auto, AfterAuto, "afterAuto", bool;
        dyl_before, DylBefore, "dylBefore", i16;
        dyl_after, DylAfter, "dylAfter", i16;
        line_spacing, LineSpacing, "lineSpacing", LineSpacing;
        keep, Keep, "keep", bool;
        keep_follow, KeepFollow, "keepFollow", bool;
        page_break_before, PageBreakBefore, "pageBreakBefore", bool;
        widow_control, WidowControl, "widowControl", bool;
        no_line_numb, NoLineNumb, "noLineNumb", bool;
        no_auto_hyph, NoAutoHyph, "noAutoHyph", bool;
        contextual_spacing, ContextualSpacing, "contextualSpacing", bool;
        mirror_indents, MirrorIndents, "mirrorIndents", bool;
        kinsoku, Kinsoku, "kinsoku", bool;
        word_wrap, WordWrap, "wordWrap", bool;
        overflow_punct, OverflowPunct, "overflowPunct", bool;
        top_line_punct, TopLinePunct, "topLinePunct", bool;
        auto_space_de, AutoSpaceDE, "autoSpaceDE", bool;
        auto_space_dn, AutoSpaceDN, "autoSpaceDN", bool;
        bidi, Bidi, "bidi", bool;
        adjust_right, AdjustRight, "adjustRight", bool;
        use_pgsu_settings, UsePgsuSettings, "usePgsuSettings", bool;
        w_align_font, WAlignFont, "wAlignFont", u16;
        ilfo, Ilfo, "ilfo", i16;
        ilvl, Ilvl, "ilvl", u8;
        out_lvl, OutLvl, "outLvl", u8;
        tabs, Tabs, "tabs", Vec<TabStop>;
        tabs_cleared, TabsCleared, "tabsCleared", Vec<i16>;
        brc_top, BrcTop, "brcTop", Brc;
        brc_left, BrcLeft, "brcLeft", Brc;
        brc_bottom, BrcBottom, "brcBottom", Brc;
        brc_right, BrcRight, "brcRight", Brc;
        brc_between, BrcBetween, "brcBetween", Brc;
        brc_bar, BrcBar, "brcBar", Brc;
        shd, Shd, "shd", Shd;
        in_table, InTable, "inTable", bool;
        ttp, Ttp, "ttp", bool;
        itap, Itap, "itap", i64;
        inner_table_cell, InnerTableCell, "innerTableCell", bool;
        inner_ttp, InnerTtp, "innerTtp", bool;
        dxa_abs, DxaAbs, "dxaAbs", i16;
        dya_abs, DyaAbs, "dyaAbs", i16;
        dxa_width, DxaWidth, "dxaWidth", i16;
        pc, Pc, "pc", u8;
        wr, Wr, "wr", u8;
        w_height_abs, WHeightAbs, "wHeightAbs", u16;
        dya_from_text, DyaFromText, "dyaFromText", i16;
        dxa_from_text, DxaFromText, "dxaFromText", i16;
        frame_locked, FrameLocked, "frameLocked", bool;
        dcs, Dcs, "dcs", u16;
        table_grpprl, TableGrpprl, "tableGrpprl", Vec<u8>;
    }
}

/// `defaultChp`: the default CHP of a document (before any style).
#[must_use]
pub fn default_chp(ftc_ascii: u16, ftc_fe: u16, ftc_other: u16) -> Chp {
    Chp {
        hps: Some(20),
        hps_bi: Some(20),
        ftc_ascii: Some(ftc_ascii),
        ftc_fe: Some(ftc_fe),
        ftc_other: Some(ftc_other),
        ftc_bi: Some(0),
        lid: Some(0x0400),
        lid_fe: Some(0x0400),
        lid_bi: Some(0x0400),
        istd: Some(10),
        ..Chp::default()
    }
}

// ───────────────────────────── character ─────────────────────────────

/// `ChpCtx`: what the application of character sprms reads and changes.
pub struct ChpCtx<'a> {
    /// the style's values (for the toggles 0x80/0x81); `sprmCIstd` and `sprmCPlain` replace it
    pub base: Chp,
    /// `sprmCIstd`: the CHP of a character style applied over the paragraph's
    pub apply_char_style: Option<&'a mut dyn FnMut(u16) -> Chp>,
    /// `sprmCPlain`, `sprmCMajority`: the CHP of the paragraph style
    pub para_style: Option<&'a Chp>,
}

impl ChpCtx<'_> {
    /// `{ base: {} }`.
    #[must_use]
    pub fn empty() -> ChpCtx<'static> {
        ChpCtx { base: Chp::default(), apply_char_style: None, para_style: None }
    }
}

/// The toggle sprms (`TOGGLES`) and their keys.
fn toggle_key(sprm: u16) -> Option<ChpKey> {
    Some(match sprm {
        0x0835 => ChpKey::Bold,
        0x0836 => ChpKey::Italic,
        0x0837 => ChpKey::Strike,
        0x0838 => ChpKey::Outline,
        0x0839 => ChpKey::Shadow,
        0x083a => ChpKey::SmallCaps,
        0x083b => ChpKey::Caps,
        0x083c => ChpKey::Vanish,
        0x0858 => ChpKey::Emboss,
        0x0854 => ChpKey::Imprint,
        0x085c => ChpKey::BoldBi,
        0x085d => ChpKey::ItalicBi,
        _ => return None,
    })
}

fn toggle_value(chp: &Chp, k: ChpKey) -> Option<bool> {
    match k {
        ChpKey::Bold => chp.bold,
        ChpKey::Italic => chp.italic,
        ChpKey::Strike => chp.strike,
        ChpKey::Outline => chp.outline,
        ChpKey::Shadow => chp.shadow,
        ChpKey::SmallCaps => chp.small_caps,
        ChpKey::Caps => chp.caps,
        ChpKey::Vanish => chp.vanish,
        ChpKey::Emboss => chp.emboss,
        ChpKey::Imprint => chp.imprint,
        ChpKey::BoldBi => chp.bold_bi,
        ChpKey::ItalicBi => chp.italic_bi,
        _ => None,
    }
}

fn set_toggle(chp: &mut Chp, k: ChpKey, v: bool) {
    let v = Some(v);
    match k {
        ChpKey::Bold => chp.bold = v,
        ChpKey::Italic => chp.italic = v,
        ChpKey::Strike => chp.strike = v,
        ChpKey::Outline => chp.outline = v,
        ChpKey::Shadow => chp.shadow = v,
        ChpKey::SmallCaps => chp.small_caps = v,
        ChpKey::Caps => chp.caps = v,
        ChpKey::Vanish => chp.vanish = v,
        ChpKey::Emboss => chp.emboss = v,
        ChpKey::Imprint => chp.imprint = v,
        ChpKey::BoldBi => chp.bold_bi = v,
        ChpKey::ItalicBi => chp.italic_bi = v,
        _ => {}
    }
}

/// `applyChp`: applies a character `Prl` (sprms of other classes are ignored).
pub fn apply_chp(chp: &mut Chp, p: &Prl<'_>, ctx: &mut ChpCtx<'_>, touched: &mut KeySet<ChpKey>) {
    if let Some(tog) = toggle_key(p.sprm) {
        let v = p.u8();
        let base_v = toggle_value(&ctx.base, tog).unwrap_or(false);
        set_toggle(
            chp,
            tog,
            if v == 0x80 {
                base_v
            } else if v == 0x81 {
                !base_v
            } else {
                v != 0
            },
        );
        touched.add(tog);
        return;
    }
    let r = p.r;
    match p.sprm {
        0x0800 => {
            chp.rmark_del = Some(p.u8() != 0);
            touched.add(ChpKey::RmarkDel);
        }
        0x0801 => {
            chp.rmark_ins = Some(p.u8() != 0);
            touched.add(ChpKey::RmarkIns);
        }
        0x0802 => {
            chp.fld_vanish = Some(p.u8() != 0);
            touched.add(ChpKey::FldVanish);
        }
        0x6a03 => {
            chp.pic_location = Some(p.i32());
            touched.add(ChpKey::PicLocation);
        }
        0x4804 => chp.ibst_rmark = Some(p.u16()),
        0x6805 => chp.dttm_rmark = Some(p.u32()),
        0x0806 => {
            chp.f_data = Some(p.u8() != 0);
            touched.add(ChpKey::FData);
        }
        0x6a09 => {
            chp.symbol = Some(Symbol { ftc: r.u16(p.op), xchar: r.u16(p.op + 2) });
            touched.add(ChpKey::Symbol);
        }
        0x080a => {
            chp.f_ole2 = Some(p.u8() != 0);
            touched.add(ChpKey::FOle2);
        }
        0x2a0c => {
            chp.highlight = Some(p.u8());
            touched.add(ChpKey::Highlight);
        }
        0x0811 => {
            chp.web_hidden = Some(p.u8() != 0);
            touched.add(ChpKey::WebHidden);
        }
        0x0818 => {
            chp.spec_vanish = Some(p.u8() != 0);
            touched.add(ChpKey::SpecVanish);
        }
        0x4a30 => {
            // sprmCIstd
            let istd = p.u16();
            if let Some(f) = ctx.apply_char_style.as_deref_mut() {
                let st = f(istd);
                // `setAll(chp, st, chp)`: the object to keep the preserved keys of is the target itself,
                // already emptied, so nothing is preserved: a plain replacement
                *chp = st.clone();
                ctx.base = st;
                touched.clear();
            }
            chp.istd = Some(istd);
            touched.add(ChpKey::Istd);
        }
        0x2a33 => {
            // sprmCPlain
            if let Some(ps) = ctx.para_style {
                *chp = ps.clone();
                ctx.base = ps.clone();
                touched.clear();
            }
        }
        0x2a34 => {
            chp.kcd = Some(p.u8());
            touched.add(ChpKey::Kcd);
        }
        0x2a3e => {
            chp.kul = Some(p.u8());
            touched.add(ChpKey::Kul);
        }
        0x8840 => {
            chp.dxa_space = Some(p.i16());
            touched.add(ChpKey::DxaSpace);
        }
        0x2a42 => {
            chp.ico = Some(p.u8());
            chp.cv = None;
            touched.add(ChpKey::Ico);
            touched.delete(ChpKey::Cv);
        }
        0x4a43 => {
            chp.hps = Some(i32::from(p.u16()));
            touched.add(ChpKey::Hps);
        }
        0x2a44 => {
            chp.hps = Some((chp.hps.unwrap_or(20) + i32::from(r.i8(p.op)) * 2).max(2));
            touched.add(ChpKey::Hps);
        }
        0x4845 => {
            chp.hps_pos = Some(p.i16());
            touched.add(ChpKey::HpsPos);
        }
        0x2a48 => {
            chp.iss = Some(p.u8());
            touched.add(ChpKey::Iss);
        }
        0x484b => {
            chp.hps_kern = Some(p.u16());
            touched.add(ChpKey::HpsKern);
        }
        0x4a4f => {
            chp.ftc_ascii = Some(p.u16());
            touched.add(ChpKey::FtcAscii);
        }
        0x4a50 => {
            chp.ftc_fe = Some(p.u16());
            touched.add(ChpKey::FtcFE);
        }
        0x4a51 => {
            chp.ftc_other = Some(p.u16());
            touched.add(ChpKey::FtcOther);
        }
        0x4852 => {
            chp.char_scale = Some(p.u16());
            touched.add(ChpKey::CharScale);
        }
        0x2a53 => {
            chp.dstrike = Some(p.u8() != 0);
            touched.add(ChpKey::Dstrike);
        }
        0x0855 => {
            chp.f_spec = Some(p.u8() != 0);
            touched.add(ChpKey::FSpec);
        }
        0x0856 => {
            chp.f_obj = Some(p.u8() != 0);
            touched.add(ChpKey::FObj);
        }
        0x2859 => {
            chp.sfx_text = Some(p.u8());
            touched.add(ChpKey::SfxText);
        }
        0x085a => {
            chp.rtl = Some(p.u8() != 0);
            touched.add(ChpKey::Rtl);
        }
        0x4a5e => {
            chp.ftc_bi = Some(p.u16());
            touched.add(ChpKey::FtcBi);
        }
        0x485f => {
            chp.lid_bi = Some(p.u16());
            touched.add(ChpKey::LidBi);
        }
        0x4a60 => {} // sprmCIcoBi
        0x4a61 => {
            chp.hps_bi = Some(p.u16());
            touched.add(ChpKey::HpsBi);
        }
        0x4863 => chp.ibst_rmark_del = Some(p.u16()),
        0x6864 => chp.dttm_rmark_del = Some(p.u32()),
        0x6865 => {
            chp.brc = brc80(r, p.op);
            touched.add(ChpKey::Brc);
        }
        0x4866 => {
            chp.shd = Some(shd80(p.u16()));
            touched.add(ChpKey::Shd);
        }
        0x286f => {
            chp.idct_hint = Some(p.u8());
            touched.add(ChpKey::IdctHint);
        }
        0x6870 => {
            // cvAuto after sprmCIco: the Word keeps the colour of the ico (measured in the Word)
            let cv = color_ref(p.u32());
            if cv == AUTO {
                chp.cv = None;
                touched.delete(ChpKey::Cv);
                if !touched.has(ChpKey::Ico) {
                    chp.ico = Some(0);
                    touched.add(ChpKey::Ico);
                }
            } else {
                chp.cv = Some(cv);
                touched.add(ChpKey::Cv);
            }
        }
        0xca71 => {
            chp.shd = Some(shd_new(r, p.op));
            touched.add(ChpKey::Shd);
        }
        0xca72 => {
            chp.brc = brc_new(r, p.op);
            touched.add(ChpKey::Brc);
        }
        0x486d | 0x4873 => {
            chp.lid = Some(p.u16());
            touched.add(ChpKey::Lid);
        }
        0x486e | 0x4874 => {
            chp.lid_fe = Some(p.u16());
            touched.add(ChpKey::LidFE);
        }
        0x0875 => {
            chp.no_proof = Some(p.u8() != 0);
            touched.add(ChpKey::NoProof);
        }
        0x6877 => {
            chp.cv_ul = Some(color_ref(p.u32()));
            touched.add(ChpKey::CvUl);
        }
        0x0882 => {
            chp.cs = Some(p.u8() != 0);
            touched.add(ChpKey::Cs);
        }
        0xca47 => {
            // sprmCMajority: properties equal to the grpprl's go back to the style
            let Some(ps) = ctx.para_style else { return };
            let mut probe = Chp::default();
            let mut dummy = KeySet::new();
            let sub = r.sub(p.op, p.op + p.len);
            for q in prls(sub) {
                apply_chp(&mut probe, &q, &mut ChpCtx::empty(), &mut dummy);
            }
            for &k in dummy.keys() {
                if chp.same_json(k, &probe) {
                    chp.copy_key(k, ps);
                    touched.delete(k);
                }
            }
        }
        _ => {}
    }
}

// ───────────────────────────── paragraph ─────────────────────────────

/// The most tab stops a paragraph keeps. The TS keeps any number (the format allows 255 per
/// operand, repeated in the grpprl); with the filter that runs per added tab, a hostile grpprl
/// costs time squared in the number of sprms. Word's own limit is far below. A tab past the limit
/// is not added (a listed difference on hostile input only, declaration 8 of F2).
pub const MAX_TABS: usize = 256;

/// `chgTabs`: `sprmPChgTabs`/`sprmPChgTabsPapx`: removes positions and adds tab stops.
fn chg_tabs(pap: &mut Pap, p: &Prl<'_>, with_close: bool) {
    let r = p.r;
    let mut o = p.op;
    let c_del = u64::from(r.u8(o));
    o += 1;
    let dels: Vec<i16> = (0..c_del).map(|i| r.i16(o + i * 2)).collect();
    // sprmPChgTabs: rgdxaClose (the tolerance) follows the deletes
    let closes: Vec<i16> = if with_close { (0..c_del).map(|i| r.i16(o + c_del * 2 + i * 2)).collect() } else { Vec::new() };
    o += c_del * 2 * if with_close { 2 } else { 1 };
    let c_add = u64::from(r.u8(o));
    o += 1;
    let adds: Vec<TabStop> = (0..c_add)
        .map(|i| {
            let tbd = r.u8(o + c_add * 2 + i);
            TabStop { pos: r.i16(o + i * 2), jc: tbd & 7, tlc: (tbd >> 3) & 7 }
        })
        .collect();
    let mut tabs: Vec<TabStop> = pap.tabs.clone().unwrap_or_default();
    // the removed positions: a Set (insertion order, no duplicates)
    let mut cleared: Vec<i16> = Vec::new();
    for c in pap.tabs_cleared.as_deref().unwrap_or(&[]) {
        if !cleared.contains(c) {
            cleared.push(*c);
        }
    }
    for (i, &d) in dels.iter().enumerate() {
        let tol = if with_close { i32::from(closes.get(i).copied().unwrap_or(0)).abs() } else { 0 };
        let before = tabs.len();
        tabs.retain(|tb| (i32::from(tb.pos) - i32::from(d)).abs() > tol);
        if (tabs.len() != before || !with_close) && !cleared.contains(&d) && cleared.len() < MAX_TABS {
            cleared.push(d);
        }
    }
    for a in adds {
        tabs.retain(|tb| tb.pos != a.pos);
        if tabs.len() < MAX_TABS {
            tabs.push(a);
        }
        cleared.retain(|&c| c != a.pos);
    }
    // the stored order is kept (the Word does not reorder when converting)
    pap.tabs = Some(tabs);
    pap.tabs_cleared = Some(cleared);
}

/// `lspd`: the line spacing of `sprmPDyaLine`.
#[must_use]
pub fn lspd(p: &Prl<'_>) -> LineSpacing {
    LineSpacing { line: p.r.i16(p.op), mult: p.r.i16(p.op + 2) != 0 }
}

/// `PapCtx`: `sprmPIstd` swaps the style (the caller decides whether to restart).
#[derive(Default)]
pub struct PapCtx<'a> {
    pub on_istd: Option<&'a mut dyn FnMut(u16)>,
}

/// `applyPap`: applies a paragraph `Prl` (sprms of other classes are ignored).
pub fn apply_pap(pap: &mut Pap, p: &Prl<'_>, touched: &mut KeySet<PapKey>, ctx: &mut PapCtx<'_>) {
    let b8 = p.u8() != 0;
    let r = p.r;
    macro_rules! t {
        ($k:ident) => {
            touched.add(PapKey::$k)
        };
    }
    match p.sprm {
        0x4600 => {
            pap.istd = p.u16();
            if let Some(f) = ctx.on_istd.as_deref_mut() {
                f(pap.istd);
            }
        }
        0x2602 => {
            // sprmPIncLvl
            let d = i32::from(r.i8(p.op));
            if (1..=9).contains(&pap.istd) {
                // istd is 1-9 here and the result is clamped to 1-9
                pap.istd = u16::try_from((i32::from(pap.istd) + d).clamp(1, 9)).unwrap_or(1);
                if let Some(f) = ctx.on_istd.as_deref_mut() {
                    f(pap.istd);
                }
            } else if pap.out_lvl.unwrap_or(9) != 9 {
                pap.out_lvl = Some(u8::try_from((i32::from(pap.out_lvl.unwrap_or(9)) + d).clamp(0, 9)).unwrap_or(0));
                t!(OutLvl);
            }
        }
        0x2403 => {
            if !touched.has(PapKey::Jc) || pap.jc.is_none() {
                pap.jc = Some(p.u8());
                t!(Jc);
            }
        }
        0x2461 => {
            pap.jc = Some(p.u8());
            t!(Jc);
        }
        0x2405 => {
            pap.keep = Some(b8);
            t!(Keep);
        }
        0x2406 => {
            pap.keep_follow = Some(b8);
            t!(KeepFollow);
        }
        0x2407 => {
            pap.page_break_before = Some(b8);
            t!(PageBreakBefore);
        }
        0x260a => {
            pap.ilvl = Some(p.u8());
            t!(Ilvl);
        }
        0x460b => {
            pap.ilfo = Some(p.i16());
            t!(Ilfo);
        }
        0x240c => {
            pap.no_line_numb = Some(b8);
            t!(NoLineNumb);
        }
        0xc60d => {
            chg_tabs(pap, p, false);
            t!(Tabs);
        }
        0xc615 => {
            chg_tabs(pap, p, true);
            t!(Tabs);
        }
        0x840e | 0x845d => {
            pap.dxa_right = Some(p.i16());
            t!(DxaRight);
            pap.dxc_right = None;
        }
        0x840f | 0x845e => {
            pap.dxa_left = Some(i64::from(p.i16()));
            t!(DxaLeft);
            pap.dxc_left = None;
        }
        0x4610 | 0x465f => {
            pap.dxa_left = Some(pap.dxa_left.unwrap_or(0) + i64::from(p.i16()));
            t!(DxaLeft);
        }
        0x8411 | 0x8460 => {
            pap.dxa_left1 = Some(p.i16());
            t!(DxaLeft1);
            pap.dxc_left1 = None;
        }
        0x4456 => {
            pap.dxc_left = Some(p.i16());
            t!(DxcLeft);
        }
        0x4455 => {
            pap.dxc_right = Some(p.i16());
            t!(DxcRight);
        }
        0x4457 => {
            pap.dxc_left1 = Some(p.i16());
            t!(DxcLeft1);
        }
        0x6412 => {
            pap.line_spacing = Some(lspd(p));
            t!(LineSpacing);
        }
        0xa413 => {
            pap.dya_before = Some(p.u16());
            t!(DyaBefore);
        }
        0xa414 => {
            pap.dya_after = Some(p.u16());
            t!(DyaAfter);
        }
        0x4458 => {
            pap.dyl_before = Some(p.i16());
            t!(DylBefore);
        }
        0x4459 => {
            pap.dyl_after = Some(p.i16());
            t!(DylAfter);
        }
        0x245b => {
            pap.before_auto = Some(b8);
            t!(BeforeAuto);
        }
        0x245c => {
            pap.after_auto = Some(b8);
            t!(AfterAuto);
        }
        0x2416 => pap.in_table = Some(b8),
        0x2417 => pap.ttp = Some(b8),
        0x6649 => pap.itap = Some(i64::from(p.i32())),
        0x664a => pap.itap = Some((pap.itap.unwrap_or(0) + i64::from(p.i32())).max(0)),
        0x244b => pap.inner_table_cell = Some(b8),
        0x244c => pap.inner_ttp = Some(b8),
        0x8418 => {
            pap.dxa_abs = Some(p.i16());
            t!(DxaAbs);
        }
        0x8419 => {
            pap.dya_abs = Some(p.i16());
            t!(DyaAbs);
        }
        0x841a => {
            pap.dxa_width = Some(p.i16());
            t!(DxaWidth);
        }
        0x261b => {
            pap.pc = Some(p.u8());
            t!(Pc);
        }
        0x2423 => {
            pap.wr = Some(p.u8());
            t!(Wr);
        }
        0x442b => {
            pap.w_height_abs = Some(p.u16());
            t!(WHeightAbs);
        }
        0x442c => {
            pap.dcs = Some(p.u16());
            t!(Dcs);
        }
        0x842e => {
            pap.dya_from_text = Some(p.i16());
            t!(DyaFromText);
        }
        0x842f => {
            pap.dxa_from_text = Some(p.i16());
            t!(DxaFromText);
        }
        0x2430 => {
            pap.frame_locked = Some(b8);
            t!(FrameLocked);
        }
        0x6424 => {
            pap.brc_top = brc80(r, p.op);
            t!(BrcTop);
        }
        0x6425 => {
            pap.brc_left = brc80(r, p.op);
            t!(BrcLeft);
        }
        0x6426 => {
            pap.brc_bottom = brc80(r, p.op);
            t!(BrcBottom);
        }
        0x6427 => {
            pap.brc_right = brc80(r, p.op);
            t!(BrcRight);
        }
        0x6428 => {
            pap.brc_between = brc80(r, p.op);
            t!(BrcBetween);
        }
        0x6629 => {
            pap.brc_bar = brc80(r, p.op);
            t!(BrcBar);
        }
        0xc64e => {
            pap.brc_top = brc_new(r, p.op);
            t!(BrcTop);
        }
        0xc64f => {
            pap.brc_left = brc_new(r, p.op);
            t!(BrcLeft);
        }
        0xc650 => {
            pap.brc_bottom = brc_new(r, p.op);
            t!(BrcBottom);
        }
        0xc651 => {
            pap.brc_right = brc_new(r, p.op);
            t!(BrcRight);
        }
        0xc652 => {
            pap.brc_between = brc_new(r, p.op);
            t!(BrcBetween);
        }
        0xc653 => {
            pap.brc_bar = brc_new(r, p.op);
            t!(BrcBar);
        }
        0x242a => {
            pap.no_auto_hyph = Some(b8);
            t!(NoAutoHyph);
        }
        0x442d => {
            pap.shd = Some(shd80(p.u16()));
            t!(Shd);
        }
        0xc64d => {
            pap.shd = Some(shd_new(r, p.op));
            t!(Shd);
        }
        0x2431 => {
            pap.widow_control = Some(b8);
            t!(WidowControl);
        }
        0x2433 => {
            pap.kinsoku = Some(b8);
            t!(Kinsoku);
        }
        0x2434 => {
            pap.word_wrap = Some(b8);
            t!(WordWrap);
        }
        0x2435 => {
            pap.overflow_punct = Some(b8);
            t!(OverflowPunct);
        }
        0x2436 => {
            pap.top_line_punct = Some(b8);
            t!(TopLinePunct);
        }
        0x2437 => {
            pap.auto_space_de = Some(b8);
            t!(AutoSpaceDE);
        }
        0x2438 => {
            pap.auto_space_dn = Some(b8);
            t!(AutoSpaceDN);
        }
        0x4439 => {
            pap.w_align_font = Some(p.u16());
            t!(WAlignFont);
        }
        0x2640 => {
            pap.out_lvl = Some(p.u8());
            t!(OutLvl);
        }
        0x2441 => {
            pap.bidi = Some(b8);
            t!(Bidi);
        }
        0x2447 => {
            pap.use_pgsu_settings = Some(b8);
            t!(UsePgsuSettings);
        }
        0x2448 => {
            pap.adjust_right = Some(b8);
            t!(AdjustRight);
        }
        0x246d => {
            pap.contextual_spacing = Some(b8);
            t!(ContextualSpacing);
        }
        0x2470 => {
            pap.mirror_indents = Some(b8);
            t!(MirrorIndents);
        }
        _ => {}
    }
}

/// `brcNone`: a border that is absent, of type 0 or nil.
#[must_use]
pub fn brc_none(b: Option<&Brc>) -> bool {
    b.is_none_or(|b| b.brc_type == 0 || b.brc_type == 0xff)
}

/// `pruneNoneBorders`: paragraph borders "none" over a base that has none either are not written
/// (as the Word).
pub fn prune_none_borders(pap: &Pap, base: &Pap, touched: &mut KeySet<PapKey>) {
    for (k, v, b) in [
        (PapKey::BrcTop, &pap.brc_top, &base.brc_top),
        (PapKey::BrcLeft, &pap.brc_left, &base.brc_left),
        (PapKey::BrcBottom, &pap.brc_bottom, &base.brc_bottom),
        (PapKey::BrcRight, &pap.brc_right, &base.brc_right),
        (PapKey::BrcBetween, &pap.brc_between, &base.brc_between),
        (PapKey::BrcBar, &pap.brc_bar, &base.brc_bar),
    ] {
        if touched.has(k) && brc_none(v.as_ref()) && brc_none(b.as_ref()) {
            touched.delete(k);
        }
    }
}

// ───────────────────────────── table ─────────────────────────────

/// A table cell (`Tc`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tc {
    pub horz_merge: u8,
    pub vert_merge: u8,
    pub vert_align: u8,
    pub text_flow: u8,
    pub fts_width: u8,
    pub w_width: i16,
    pub fit_text: bool,
    pub no_wrap: bool,
    pub hide_mark: bool,
    pub brc_top: Option<Brc>,
    pub brc_left: Option<Brc>,
    pub brc_bottom: Option<Brc>,
    pub brc_right: Option<Brc>,
    /// borders defined explicitly (a non-nil TC80 or `sprmTSetBrc`)
    pub brc_set: [bool; 4],
    pub shd: Option<Shd>,
    /// cell margins (`sprmTCellPadding`), twips: top, left, bottom, right
    pub mar: [Option<i16>; 4],
}

impl Tc {
    /// `newTc`.
    #[must_use]
    pub fn new() -> Tc {
        Tc {
            horz_merge: 0,
            vert_merge: 0,
            vert_align: 0,
            text_flow: 0,
            fts_width: 0,
            w_width: 0,
            fit_text: false,
            no_wrap: false,
            hide_mark: false,
            brc_top: None,
            brc_left: None,
            brc_bottom: None,
            brc_right: None,
            brc_set: [false; 4],
            shd: None,
            mar: [None; 4],
        }
    }
}

impl Default for Tc {
    fn default() -> Self {
        Tc::new()
    }
}

/// The table borders (`TableBorders`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TableBorders {
    pub top: Option<Brc>,
    pub left: Option<Brc>,
    pub bottom: Option<Brc>,
    pub right: Option<Brc>,
    pub inside_h: Option<Brc>,
    pub inside_v: Option<Brc>,
}

/// A width with its type (`{ fts, w }`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Width {
    pub fts: u8,
    pub w: i16,
}

/// `tlp`: the table look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tlp {
    pub itl: i16,
    pub grfatl: u16,
}

/// The most work (elements visited) the structural sprms of one table row may cost. The TS has no
/// bound: `sprmTInsert` adds up to 255 cells per sprm and `sprmTDxaCol` shifts every following
/// column, so a grpprl of thousands of sprms costs time squared in the number of cells and
/// memory in the number of cells. Word's own limit is 63 columns. Past the budget the structural
/// sprms are ignored (a listed difference on hostile input only, declaration 8 of F2).
pub const MAX_TAP_WORK: u64 = 1 << 24;

/// The most cells a table row keeps (the same listed difference).
pub const MAX_TAP_CELLS: usize = 1 << 12;

/// The table properties of a row (`Tap`).
#[derive(Debug, Clone, PartialEq)]
pub struct Tap {
    pub jc: Option<u16>,
    pub dxa_left: Option<i16>,
    pub dxa_gap_half: Option<i16>,
    pub row_height: Option<i16>,
    pub cant_split: Option<bool>,
    pub header: Option<bool>,
    pub borders: TableBorders,
    pub borders_set: bool,
    pub centers: Vec<i64>,
    pub tcs: Vec<Tc>,
    pub table_width: Option<Width>,
    pub width_before: Option<Width>,
    pub width_after: Option<Width>,
    pub width_indent: Option<Width>,
    pub autofit: Option<bool>,
    /// top, left, bottom, right
    pub padding: [Option<i16>; 4],
    pub spacing: Option<i16>,
    pub istd: Option<u16>,
    pub tlp: Option<Tlp>,
    pub shd_table: Option<Shd>,
    pub bidi: Option<bool>,
    pub pc: Option<u8>,
    pub dxa_abs: Option<i16>,
    pub dya_abs: Option<i16>,
    pub dxa_from_text: Option<i16>,
    pub dya_from_text: Option<i16>,
    pub dxa_from_text_right: Option<i16>,
    pub dya_from_text_bottom: Option<i16>,
    pub no_allow_overlap: Option<bool>,
    /// cells created only by `sprmTInsert` (no `sprmTDefTable`): the final position is in `finish_tap`
    pub insert_built: Option<bool>,
    /// the shadings by cell (`sprmTDefTableShd*`, in priority order)
    pub shd_raw: Option<Vec<Shd>>,
    pub shd_def: Option<Vec<Shd>>,
    pub shd80: Option<Vec<Shd>>,
    work_left: u64,
}

impl Tap {
    /// `newTap`.
    #[must_use]
    pub fn new() -> Tap {
        Tap {
            jc: None,
            dxa_left: None,
            dxa_gap_half: None,
            row_height: None,
            cant_split: None,
            header: None,
            borders: TableBorders::default(),
            borders_set: false,
            centers: Vec::new(),
            tcs: Vec::new(),
            table_width: None,
            width_before: None,
            width_after: None,
            width_indent: None,
            autofit: None,
            padding: [None; 4],
            spacing: None,
            istd: None,
            tlp: None,
            shd_table: None,
            bidi: None,
            pc: None,
            dxa_abs: None,
            dya_abs: None,
            dxa_from_text: None,
            dya_from_text: None,
            dxa_from_text_right: None,
            dya_from_text_bottom: None,
            no_allow_overlap: None,
            insert_built: None,
            shd_raw: None,
            shd_def: None,
            shd80: None,
            work_left: MAX_TAP_WORK,
        }
    }

    /// Spends `n` units of the work budget; false (and nothing spent) when it is exhausted.
    fn spend(&mut self, n: usize) -> bool {
        let n = n as u64;
        if n > self.work_left {
            self.work_left = 0;
            return false;
        }
        self.work_left -= n;
        true
    }
}

impl Default for Tap {
    fn default() -> Self {
        Tap::new()
    }
}

/// `readTc80`.
fn read_tc80(r: ByteReader<'_>, o: u64) -> Tc {
    let mut tc = Tc::new();
    let grf = r.u16(o);
    tc.horz_merge = (grf & 3) as u8;
    tc.text_flow = ((grf >> 2) & 7) as u8;
    tc.vert_merge = ((grf >> 5) & 3) as u8;
    tc.vert_align = ((grf >> 7) & 3) as u8;
    tc.fts_width = ((grf >> 9) & 7) as u8;
    tc.fit_text = grf & 0x1000 != 0;
    tc.no_wrap = grf & 0x2000 != 0;
    tc.hide_mark = grf & 0x4000 != 0;
    tc.w_width = r.i16(o + 2);
    // brcType 0 in the TC80 = inherits the borders of the row/table (the Word 2000+ writes it so)
    for i in 0..4u64 {
        if let Some(b) = brc80(r, o + 4 + i * 4)
            && b.brc_type != 0
        {
            match i {
                0 => tc.brc_top = Some(b),
                1 => tc.brc_left = Some(b),
                2 => tc.brc_bottom = Some(b),
                _ => tc.brc_right = Some(b),
            }
            tc.brc_set[i as usize] = true;
        }
    }
    tc
}

/// `forCells`: `itcFirst..min(itcLim, cells)`.
fn for_cells(tap: &mut Tap, first: u8, lim: u8, mut f: impl FnMut(&mut Tc, usize)) {
    let end = usize::from(lim).min(tap.tcs.len());
    for i in usize::from(first)..end {
        f(&mut tap.tcs[i], i);
    }
}

/// `shdArray`: the shadings of an operand, `size` bytes each.
fn shd_array(p: &Prl<'_>, size: u64, read: impl Fn(u64) -> Shd) -> Vec<Shd> {
    let mut out = Vec::new();
    let mut o = p.op;
    while o + size <= p.op + p.len {
        out.push(read(o));
        o += size;
    }
    out
}

/// `finishTap`: rows built by `sprmTInsert` start at `dxaLeft - dxaGapHalf`.
pub fn finish_tap(tap: &mut Tap) {
    if tap.insert_built == Some(true)
        && let Some(&first) = tap.centers.first()
    {
        let shift = i64::from(tap.dxa_left.unwrap_or(0)) - i64::from(tap.dxa_gap_half.unwrap_or(0)) - first;
        if shift != 0 {
            for c in &mut tap.centers {
                *c += shift;
            }
        }
    }
}

fn table_sides_80(r: ByteReader<'_>, op: u64) -> TableBorders {
    TableBorders {
        top: brc80(r, op),
        left: brc80(r, op + 4),
        bottom: brc80(r, op + 8),
        right: brc80(r, op + 12),
        inside_h: brc80(r, op + 16),
        inside_v: brc80(r, op + 20),
    }
}

fn table_sides_new(r: ByteReader<'_>, op: u64) -> TableBorders {
    TableBorders {
        top: brc_new(r, op),
        left: brc_new(r, op + 8),
        bottom: brc_new(r, op + 16),
        right: brc_new(r, op + 24),
        inside_h: brc_new(r, op + 32),
        inside_v: brc_new(r, op + 40),
    }
}

/// `Array.prototype.slice(0, n)` of an optional list, then the new items.
fn slice_then(old: &Option<Vec<Shd>>, n: usize, new: Vec<Shd>) -> Vec<Shd> {
    let mut v: Vec<Shd> = old.as_deref().unwrap_or(&[]).iter().take(n).copied().collect();
    v.extend(new);
    v
}

/// `applyTap`: applies a table `Prl` to the row (sprms of other classes are ignored).
pub fn apply_tap(tap: &mut Tap, p: &Prl<'_>) {
    let r = p.r;
    let op = p.op;
    match p.sprm {
        0x5400 | 0x548a => tap.jc = Some(p.u16()),
        0x9601 => tap.dxa_left = Some(p.i16()),
        0x9602 => tap.dxa_gap_half = Some(p.i16()),
        // sprmTFCantSplit90 (0x3403) is ignored in favour of sprmTFCantSplit (measured in the Word)
        0x3466 => tap.cant_split = Some(p.u8() != 0),
        0x3404 => tap.header = Some(p.u8() != 0),
        0xd605 => {
            // sprmTTableBorders80: 6 x Brc80 (the operand has a cb)
            tap.borders = table_sides_80(r, op);
            tap.borders_set = true;
        }
        0xd613 => {
            tap.borders = table_sides_new(r, op);
            tap.borders_set = true;
        }
        0x9407 => tap.row_height = Some(p.i16()),
        0xd608 | 0xd606 => {
            // sprmTDefTable
            let n = u64::from(r.u8(op));
            let centers: Vec<i64> = (0..=n).map(|i| i64::from(r.i16(op + 1 + i * 2))).collect();
            let tc_start = op + 1 + (n + 1) * 2;
            let tcs: Vec<Tc> = (0..n)
                .map(|i| {
                    let o = tc_start + i * 20;
                    if o + 20 <= op + p.len { read_tc80(r, o) } else { Tc::new() }
                })
                .collect();
            tap.centers = centers;
            tap.tcs = tcs;
            tap.insert_built = Some(false);
        }
        0xd609 => tap.shd80 = Some(shd_array(p, 2, |o| shd80(r.u16(o)))),
        0xd612 => tap.shd_def = Some(shd_array(p, 10, |o| shd_new(r, o))),
        0xd616 => tap.shd_def = Some(slice_then(&tap.shd_def, 22, shd_array(p, 10, |o| shd_new(r, o)))),
        0xd60c => tap.shd_def = Some(slice_then(&tap.shd_def, 44, shd_array(p, 10, |o| shd_new(r, o)))),
        0xd670 => tap.shd_raw = Some(shd_array(p, 10, |o| shd_new(r, o))),
        0xd671 => tap.shd_raw = Some(slice_then(&tap.shd_raw, 22, shd_array(p, 10, |o| shd_new(r, o)))),
        0xd672 => tap.shd_raw = Some(slice_then(&tap.shd_raw, 44, shd_array(p, 10, |o| shd_new(r, o)))),
        0x740a => tap.tlp = Some(Tlp { itl: r.i16(op), grfatl: r.u16(op + 2) }),
        0x560b | 0x5664 => tap.bidi = Some(p.u16() != 0),
        0x360d => tap.pc = Some(p.u8()),
        0x940e => tap.dxa_abs = Some(p.i16()),
        0x940f => tap.dya_abs = Some(p.i16()),
        0x9410 => tap.dxa_from_text = Some(p.i16()),
        0x9411 => tap.dya_from_text = Some(p.i16()),
        0x941e => tap.dxa_from_text_right = Some(p.i16()),
        0x941f => tap.dya_from_text_bottom = Some(p.i16()),
        0x3465 => tap.no_allow_overlap = Some(p.u8() != 0),
        0xf614 => tap.table_width = Some(Width { fts: r.u8(op), w: r.i16(op + 1) }),
        0xf617 => tap.width_before = Some(Width { fts: r.u8(op), w: r.i16(op + 1) }),
        0xf618 => tap.width_after = Some(Width { fts: r.u8(op), w: r.i16(op + 1) }),
        0xf661 => tap.width_indent = Some(Width { fts: r.u8(op), w: r.i16(op + 1) }),
        0x3615 => tap.autofit = Some(p.u8() != 0),
        0x563a => tap.istd = Some(p.u16()),
        0xd660 => tap.shd_table = Some(shd_new(r, op)),
        0x7621 => {
            // sprmTInsert: itcInsert, ctc, dxaCol
            let (itc, ctc, dxa) = (usize::from(r.u8(op)), usize::from(r.u8(op + 1)), i64::from(r.i16(op + 2)));
            if tap.centers.is_empty() {
                tap.centers = vec![0];
                tap.insert_built = Some(true);
            }
            for k in 0..ctc {
                if tap.tcs.len() >= MAX_TAP_CELLS || !tap.spend(tap.centers.len()) {
                    break;
                }
                let at = (itc + k).min(tap.tcs.len());
                tap.tcs.insert(at, Tc::new());
                let left = tap.centers.get(at).copied().or_else(|| tap.centers.last().copied()).unwrap_or(0);
                // `splice(at + 1, 0, ...)` clamps the index to the length
                let pos = (at + 1).min(tap.centers.len());
                tap.centers.insert(pos, left + dxa);
                for c in tap.centers.iter_mut().skip(at + 2) {
                    *c += dxa;
                }
            }
        }
        0x5622 => {
            // sprmTDelete
            let (first, lim) = (usize::from(r.u8(op)), usize::from(r.u8(op + 1)));
            let cnt = lim.min(tap.tcs.len()).saturating_sub(first);
            if cnt > 0 && tap.tcs.len() - cnt >= 1 && tap.spend(tap.centers.len()) {
                let w = tap.centers.get(first + cnt).copied().unwrap_or(0) - tap.centers.get(first).copied().unwrap_or(0);
                tap.tcs.drain(first..first + cnt);
                // `centers.splice(first + 1, cnt)`
                let from = (first + 1).min(tap.centers.len());
                let to = (first + 1 + cnt).min(tap.centers.len());
                tap.centers.drain(from..to);
                for c in tap.centers.iter_mut().skip(first + 1) {
                    *c -= w;
                }
            }
        }
        0x7623 => {
            // sprmTDxaCol: itcFirst, itcLim, dxaCol
            let (first, lim, dxa) = (usize::from(r.u8(op)), usize::from(r.u8(op + 1)), i64::from(r.i16(op + 2)));
            for i in first..lim.min(tap.tcs.len()) {
                if !tap.spend(tap.centers.len()) {
                    break;
                }
                let delta = dxa - (tap.centers.get(i + 1).copied().unwrap_or(0) - tap.centers.get(i).copied().unwrap_or(0));
                for c in tap.centers.iter_mut().skip(i + 1) {
                    *c += delta;
                }
            }
        }
        0x5624 => {
            // sprmTMerge
            let (first, lim) = (r.u8(op), r.u8(op + 1));
            for_cells(tap, first, lim, |tc, i| tc.horz_merge = if i == usize::from(first) { 3 } else { 1 });
        }
        0x5625 => {
            let (first, lim) = (r.u8(op), r.u8(op + 1));
            for_cells(tap, first, lim, |tc, _| tc.horz_merge = 0);
        }
        0xd620 | 0xd62f => {
            // sprmTSetBrc80 / sprmTSetBrc: cb, itcFirst, itcLim, bordersToApply, brc
            let (first, lim, which) = (r.u8(op), r.u8(op + 1), r.u8(op + 2));
            let b = if p.sprm == 0xd620 { brc80(r, op + 3) } else { brc_new(r, op + 3) };
            for_cells(tap, first, lim, |tc, _| {
                if which & 1 != 0 {
                    tc.brc_top = b;
                    tc.brc_set[0] = true;
                }
                if which & 2 != 0 {
                    tc.brc_left = b;
                    tc.brc_set[1] = true;
                }
                if which & 4 != 0 {
                    tc.brc_bottom = b;
                    tc.brc_set[2] = true;
                }
                if which & 8 != 0 {
                    tc.brc_right = b;
                    tc.brc_set[3] = true;
                }
            });
        }
        0x7629 => {
            // sprmTTextFlow: itcFirst, itcLim, grpfTFlow (u16)
            let (first, lim, flow) = (r.u8(op), r.u8(op + 1), r.u16(op + 2));
            for_cells(tap, first, lim, |tc, _| tc.text_flow = (flow & 7) as u8);
        }
        0xd62b => {
            // sprmTVertMerge: itc, vertMergeFlags
            let (itc, v) = (usize::from(r.u8(op)), r.u8(op + 1));
            if let Some(tc) = tap.tcs.get_mut(itc) {
                tc.vert_merge = v;
            }
        }
        0xd62c => {
            // sprmTVertAlign: itcFirst, itcLim, va
            let (first, lim, va) = (r.u8(op), r.u8(op + 1), r.u8(op + 2));
            for_cells(tap, first, lim, |tc, _| tc.vert_align = va);
        }
        0xd62d | 0xd62e => {
            // sprmTSetShd/Odd (TableShadeOperand: itcFirst, itcLim, shd)
            let (first, lim) = (r.u8(op), r.u8(op + 1));
            let s = shd_new(r, op + 2);
            let all = p.sprm == 0xd62d;
            for_cells(tap, first, lim, |tc, i| {
                if all || (i - usize::from(first)) % 2 == 0 {
                    tc.shd = Some(s);
                }
            });
        }
        0xd632..=0xd634 => {
            // CSSA: itcFirst, itcLim, grfbrc, ftsWidth, wWidth
            let (first, lim, grf, fts, w) = (r.u8(op), r.u8(op + 1), r.u8(op + 2), r.u8(op + 3), r.i16(op + 4));
            if p.sprm == 0xd633 {
                if fts == 3 || fts == 0x13 {
                    tap.spacing = Some(w);
                }
                return;
            }
            // ftsNil with width 0 = margin zero (the Word writes 0 in the .docx)
            let set = |m: &mut [Option<i16>; 4]| {
                for (bit, slot) in m.iter_mut().enumerate() {
                    if grf & (1 << bit) != 0 {
                        *slot = Some(w);
                    }
                }
            };
            if p.sprm == 0xd634 {
                set(&mut tap.padding);
            } else {
                for_cells(tap, first, lim, |tc, _| set(&mut tc.mar));
            }
        }
        0xd635 => {
            // sprmTCellWidth: itcFirst, itcLim, ftsWidth, wWidth
            let (first, lim, fts, w) = (r.u8(op), r.u8(op + 1), r.u8(op + 2), r.i16(op + 3));
            for_cells(tap, first, lim, |tc, _| {
                tc.fts_width = fts;
                tc.w_width = w;
            });
        }
        0xd642 => {
            // sprmTCellFHideMark
            let (first, lim, v) = (r.u8(op), r.u8(op + 1), r.u8(op + 2));
            for_cells(tap, first, lim, |tc, _| tc.hide_mark = v != 0);
        }
        0xd639 => {
            // sprmTFCellNoWrap
            let (first, lim, v) = (r.u8(op), r.u8(op + 1), r.u8(op + 2));
            for_cells(tap, first, lim, |tc, _| tc.no_wrap = v != 0);
        }
        _ => {}
    }
}

// ───────────────────────────── section ─────────────────────────────

/// The section properties (`Sep`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sep {
    pub bkc: u8,
    pub title_page: bool,
    pub ccol_m1: u16,
    pub dxa_columns: i16,
    pub evenly_spaced: bool,
    /// `colWidths[i] = ...` of a JS array: indexes not set are holes (`None`)
    pub col_widths: Vec<Option<i16>>,
    pub col_spacings: Vec<Option<i16>>,
    pub l_between: bool,
    pub xa_page: u16,
    pub ya_page: u16,
    /// 1 portrait, 2 landscape
    pub orientation: u8,
    pub dxa_left: u16,
    pub dxa_right: u16,
    pub dya_top: i16,
    pub dya_bottom: i16,
    pub dza_gutter: u16,
    pub dya_hdr_top: u16,
    pub dya_hdr_bottom: u16,
    pub nfc_pgn: u8,
    pub pgn_restart: bool,
    pub pgn_start: u32,
    pub vjc: u8,
    pub lnn_mod: u16,
    pub dxa_lnn: i16,
    pub lnc: u8,
    pub lnn_min: u16,
    pub brc_top: Option<Brc>,
    pub brc_left: Option<Brc>,
    pub brc_bottom: Option<Brc>,
    pub brc_right: Option<Brc>,
    pub pgb_prop: u16,
    pub clm: u16,
    pub dya_line_pitch: Option<i16>,
    pub dxt_char_space: i32,
    pub text_flow: u16,
    pub bidi: bool,
    pub rtl_gutter: bool,
    pub fpc: Option<u8>,
    pub rnc_ftn: Option<u8>,
    pub n_ftn: Option<u16>,
    pub nfc_ftn_ref: Option<u16>,
    pub rnc_edn: Option<u8>,
    pub n_edn: Option<u16>,
    pub nfc_edn_ref: Option<u16>,
    pub f_endnote: bool,
    pub dm_bin_first: u16,
    pub dm_bin_other: u16,
    pub dm_paper_req: Option<u16>,
    pub protected_sec: Option<bool>,
    pub cns_pgn: Option<u8>,
    pub heading_pgn: Option<u8>,
}

/// `defaultSep`.
#[must_use]
pub fn default_sep() -> Sep {
    Sep {
        bkc: 2,
        title_page: false,
        ccol_m1: 0,
        dxa_columns: 720,
        evenly_spaced: true,
        col_widths: Vec::new(),
        col_spacings: Vec::new(),
        l_between: false,
        xa_page: 12240,
        ya_page: 15840,
        orientation: 1,
        dxa_left: 1800,
        dxa_right: 1800,
        dya_top: 1440,
        dya_bottom: 1440,
        dza_gutter: 0,
        dya_hdr_top: 720,
        dya_hdr_bottom: 720,
        nfc_pgn: 0,
        pgn_restart: false,
        pgn_start: 1,
        vjc: 0,
        lnn_mod: 0,
        dxa_lnn: 0,
        lnc: 0,
        lnn_min: 0,
        brc_top: None,
        brc_left: None,
        brc_bottom: None,
        brc_right: None,
        pgb_prop: 0,
        clm: 0,
        dya_line_pitch: None,
        dxt_char_space: 0,
        text_flow: 0,
        bidi: false,
        rtl_gutter: false,
        fpc: None,
        rnc_ftn: None,
        n_ftn: None,
        nfc_ftn_ref: None,
        rnc_edn: None,
        n_edn: None,
        nfc_edn_ref: None,
        f_endnote: true,
        dm_bin_first: 0,
        dm_bin_other: 0,
        dm_paper_req: None,
        protected_sec: None,
        cns_pgn: None,
        heading_pgn: None,
    }
}

/// `arr[i] = v` of a JS array: the array grows to `i + 1` with holes.
fn set_at(v: &mut Vec<Option<i16>>, i: usize, x: i16) {
    if v.len() <= i {
        v.resize(i + 1, None);
    }
    v[i] = Some(x);
}

/// `applySep`: applies a section `Prl` (sprms of other classes are ignored).
pub fn apply_sep(sep: &mut Sep, p: &Prl<'_>) {
    let r = p.r;
    let op = p.op;
    match p.sprm {
        0x3000 => sep.cns_pgn = Some(p.u8()),
        0x3001 => sep.heading_pgn = Some(p.u8()),
        0xf203 => set_at(&mut sep.col_widths, usize::from(r.u8(op)), r.i16(op + 1)),
        0xf204 => set_at(&mut sep.col_spacings, usize::from(r.u8(op)), r.i16(op + 1)),
        0x3005 => sep.evenly_spaced = p.u8() != 0,
        0x3006 => sep.protected_sec = Some(p.u8() != 0),
        0x5007 => sep.dm_bin_first = p.u16(),
        0x5008 => sep.dm_bin_other = p.u16(),
        0x5026 => sep.dm_paper_req = Some(p.u16()),
        0x3009 => sep.bkc = p.u8(),
        0x300a => sep.title_page = p.u8() != 0,
        0x500b => sep.ccol_m1 = p.u16(),
        0x900c => sep.dxa_columns = p.i16(),
        0x300e => sep.nfc_pgn = p.u8(),
        0x3011 => sep.pgn_restart = p.u8() != 0,
        0x3012 => sep.f_endnote = p.u8() != 0,
        0x3013 => sep.lnc = p.u8(),
        0x5015 => sep.lnn_mod = p.u16(),
        0x9016 => sep.dxa_lnn = p.i16(),
        0xb017 => sep.dya_hdr_top = p.u16(),
        0xb018 => sep.dya_hdr_bottom = p.u16(),
        0x3019 => sep.l_between = p.u8() != 0,
        0x301a => sep.vjc = p.u8(),
        0x501b => sep.lnn_min = p.u16(),
        0x501c => sep.pgn_start = u32::from(p.u16()),
        0x7044 => sep.pgn_start = p.u32(),
        0x301d => sep.orientation = p.u8(),
        0xb01f => sep.xa_page = p.u16(),
        0xb020 => sep.ya_page = p.u16(),
        0xb021 => sep.dxa_left = p.u16(),
        0xb022 => sep.dxa_right = p.u16(),
        0x9023 => sep.dya_top = p.i16(),
        0x9024 => sep.dya_bottom = p.i16(),
        0xb025 => sep.dza_gutter = p.u16(),
        0x3228 => sep.bidi = p.u8() != 0,
        0x322a => sep.rtl_gutter = p.u8() != 0,
        0x702b => sep.brc_top = brc80(r, op),
        0x702c => sep.brc_left = brc80(r, op),
        0x702d => sep.brc_bottom = brc80(r, op),
        0x702e => sep.brc_right = brc80(r, op),
        0xd234 => sep.brc_top = brc_new(r, op),
        0xd235 => sep.brc_left = brc_new(r, op),
        0xd236 => sep.brc_bottom = brc_new(r, op),
        0xd237 => sep.brc_right = brc_new(r, op),
        0x522f => sep.pgb_prop = p.u16(),
        0x7030 => sep.dxt_char_space = p.i32(),
        0x9031 => sep.dya_line_pitch = Some(p.i16()),
        0x5032 => sep.clm = p.u16(),
        0x5033 => sep.text_flow = p.u16(),
        0x303b => sep.fpc = Some(p.u8()),
        0x303c => sep.rnc_ftn = Some(p.u8()),
        0x303e => sep.rnc_edn = Some(p.u8()),
        0x503f => sep.n_ftn = Some(p.u16()),
        0x5040 => sep.nfc_ftn_ref = Some(p.u16()),
        0x5041 => sep.n_edn = Some(p.u16()),
        0x5042 => sep.nfc_edn_ref = Some(p.u16()),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One `Prl` of `sprm` with an operand, over a grpprl built here.
    fn run<R>(sprm: u16, operand: &[u8], f: impl FnOnce(Prl<'_>) -> R) -> R {
        let mut g = sprm.to_le_bytes().to_vec();
        if sprm >> 13 == 6 {
            g.push(u8::try_from(operand.len()).unwrap());
        }
        g.extend(operand);
        f(prls(&g).next().unwrap())
    }

    fn chp_of(sprms: &[(u16, &[u8])]) -> (Chp, KeySet<ChpKey>) {
        let mut chp = Chp::default();
        let mut t = KeySet::new();
        for (s, o) in sprms {
            run(*s, o, |p| apply_chp(&mut chp, &p, &mut ChpCtx::empty(), &mut t));
        }
        (chp, t)
    }

    #[test]
    fn colours_and_borders() {
        assert_eq!(color_ref(0xff00_0000), AUTO);
        assert_eq!(color_ref(0x0011_2233), 0x33_22_11);
        assert_eq!(ico_color(0), AUTO);
        assert_eq!(ico_color(2), 0x0000ff);
        assert_eq!(ico_color(17), AUTO);
        let nil = [0xff, 0xff, 0xff, 0xff];
        assert!(brc80(ByteReader::new(&nil), 0).is_none());
        let b = brc80(ByteReader::new(&[4, 1, 6, 0x65]), 0).unwrap();
        assert_eq!((b.width, b.brc_type, b.color, b.space, b.shadow, b.frame, b.from_new), (4, 1, 0xff0000, 5, true, true, false));
        let n = brc_new(ByteReader::new(&[1, 2, 3, 0, 8, 7, 0x21, 0]), 0).unwrap();
        assert_eq!((n.color, n.width, n.brc_type, n.space, n.shadow, n.from_new), (0x010203, 8, 7, 1, true, true));
        assert!(brc_new(ByteReader::new(&[0xff; 8]), 0).is_none());
        assert_eq!(shd80(0xfc00).pat, 0xffff);
        assert_eq!(shd80(0x0441), Shd { fore: 0x000000, back: 0x0000ff, pat: 1 });
    }

    #[test]
    fn toggles_resolve_against_the_base() {
        let mut chp = Chp::default();
        let mut ctx = ChpCtx::empty();
        ctx.base.bold = Some(true);
        let mut t = KeySet::new();
        for v in [0x80u8, 0x81, 0x00, 0x05] {
            run(0x0835, &[v], |p| apply_chp(&mut chp, &p, &mut ctx, &mut t));
        }
        assert_eq!(chp.bold, Some(true));
        run(0x0835, &[0x81], |p| apply_chp(&mut chp, &p, &mut ctx, &mut t));
        assert_eq!(chp.bold, Some(false));
        assert_eq!(t.keys(), &[ChpKey::Bold]);
    }

    #[test]
    fn size_increment_floor_and_colour_rules() {
        let (c, _) = chp_of(&[(0x4a43, &[10, 0]), (0x2a44, &[0xf0])]);
        assert_eq!(c.hps, Some(2));
        let (c, t) = chp_of(&[(0x2a44, &[3])]);
        assert_eq!(c.hps, Some(26));
        assert_eq!(t.keys(), &[ChpKey::Hps]);
        // ico clears cv; a later cvAuto keeps the ico colour and marks ico when it was not marked
        let (c, t) = chp_of(&[(0x6870, &[1, 2, 3, 0]), (0x2a42, &[2])]);
        assert_eq!((c.ico, c.cv), (Some(2), None));
        assert_eq!(t.keys(), &[ChpKey::Ico]);
        let (c, t) = chp_of(&[(0x6870, &[1, 2, 3, 0]), (0x6870, &[0, 0, 0, 0xff])]);
        assert_eq!((c.ico, c.cv), (Some(0), None));
        assert_eq!(t.keys(), &[ChpKey::Ico]);
    }

    #[test]
    fn character_style_and_plain_replace_the_chp_and_reset_the_touched_set() {
        let ps = Chp { hps: Some(24), bold: Some(true), ..Chp::default() };
        let mut chp = Chp { italic: Some(true), ..ps.clone() };
        let mut t = KeySet::new();
        t.add(ChpKey::Italic);
        let mut f = |istd: u16| Chp { istd: Some(istd), hps: Some(30), ..Chp::default() };
        let mut ctx = ChpCtx { base: ps.clone(), apply_char_style: Some(&mut f), para_style: Some(&ps) };
        run(0x4a30, &[7, 0], |p| apply_chp(&mut chp, &p, &mut ctx, &mut t));
        assert_eq!((chp.hps, chp.istd, chp.bold, chp.italic), (Some(30), Some(7), None, None));
        assert_eq!(t.keys(), &[ChpKey::Istd]);
        assert_eq!(ctx.base.hps, Some(30));
        run(0x2a33, &[0], |p| apply_chp(&mut chp, &p, &mut ctx, &mut t));
        assert_eq!(chp, ps);
        assert!(t.is_empty());
    }

    #[test]
    fn majority_sends_equal_values_back_to_the_style() {
        let ps = Chp { hps: Some(24), bold: Some(true), ..Chp::default() };
        let mut chp = Chp { hps: Some(24), bold: Some(false), ..Chp::default() };
        let mut t = KeySet::new();
        t.add(ChpKey::Hps);
        t.add(ChpKey::Bold);
        let mut ctx = ChpCtx { base: Chp::default(), apply_char_style: None, para_style: Some(&ps) };
        // the grpprl: size 24 (equal to chp), bold on (different from chp)
        run(0xca47, &[0x43, 0x4a, 24, 0, 0x35, 0x08, 1], |p| apply_chp(&mut chp, &p, &mut ctx, &mut t));
        assert_eq!(t.keys(), &[ChpKey::Bold]);
        assert_eq!(chp.hps, Some(24));
        // without a paragraph style nothing happens
        let mut chp2 = Chp::default();
        run(0xca47, &[0x35, 0x08, 1], |p| apply_chp(&mut chp2, &p, &mut ChpCtx::empty(), &mut t));
        assert_eq!(chp2, Chp::default());
        // a border read by brcNew never equals one read by brc80 (the key order of the TS objects)
        let b80 = Brc { color: 1, width: 2, brc_type: 1, space: 0, shadow: false, frame: false, from_new: false };
        assert_ne!(b80, Brc { from_new: true, ..b80 });
    }

    #[test]
    fn key_sets_keep_insertion_order() {
        let mut s = KeySet::new();
        s.add(PapKey::Jc);
        s.add(PapKey::Keep);
        s.add(PapKey::Jc);
        assert_eq!(s.keys(), &[PapKey::Jc, PapKey::Keep]);
        s.delete(PapKey::Jc);
        s.add(PapKey::Jc);
        assert_eq!(s.keys(), &[PapKey::Keep, PapKey::Jc]);
        assert_eq!(ChpKey::ALL.len(), 56);
        assert_eq!(PapKey::ALL.len(), 60);
        assert_eq!(ChpKey::FtcFE.name(), "ftcFE");
        assert_eq!(PapKey::AutoSpaceDE.name(), "autoSpaceDE");
    }

    fn pap_of(init: Pap, sprms: &[(u16, &[u8])]) -> (Pap, KeySet<PapKey>, Vec<u16>) {
        let mut pap = init;
        let mut t = KeySet::new();
        let mut istds = Vec::new();
        {
            let mut rec = |v: u16| istds.push(v);
            let mut ctx = PapCtx { on_istd: Some(&mut rec) };
            for (s, o) in sprms {
                run(*s, o, |p| apply_pap(&mut pap, &p, &mut t, &mut ctx));
            }
        }
        (pap, t, istds)
    }

    #[test]
    fn paragraph_style_levels_and_relative_indents() {
        let (p, t, istds) = pap_of(Pap { istd: 2, ..Pap::default() }, &[(0x2602, &[0xff]), (0x2602, &[20]), (0x4600, &[5, 0])]);
        assert_eq!((p.istd, istds), (5, vec![1, 9, 5]));
        assert!(t.is_empty());
        // outside the heading styles the outline level moves (clamped to 0-9) when it is not 9
        let (p, t, _) = pap_of(Pap { istd: 20, out_lvl: Some(3), ..Pap::default() }, &[(0x2602, &[0xfa])]);
        assert_eq!(p.out_lvl, Some(0));
        assert_eq!(t.keys(), &[PapKey::OutLvl]);
        let (p, _, _) = pap_of(Pap { istd: 20, ..Pap::default() }, &[(0x2602, &[1])]);
        assert_eq!(p.out_lvl, None);
        // sprmPDxaLeft (absolute) clears dxcLeft; the relative one adds
        let (p, _, _) = pap_of(Pap::default(), &[(0x4456, &[5, 0]), (0x840f, &[100, 0]), (0x4610, &[0xf0, 0xff]), (0x4610, &[0x10, 0])]);
        assert_eq!((p.dxa_left, p.dxc_left), (Some(100), None));
        // itap never goes below zero
        let (p, _, _) = pap_of(Pap::default(), &[(0x6649, &[3, 0, 0, 0]), (0x664a, &[0xf0, 0xff, 0xff, 0xff])]);
        assert_eq!(p.itap, Some(0));
        // the second justification sprm only sets a missing or untouched jc
        let (p, _, _) = pap_of(Pap::default(), &[(0x2461, &[2]), (0x2403, &[1])]);
        assert_eq!(p.jc, Some(2));
    }

    #[test]
    fn tab_stops_are_changed_and_cleared() {
        let init = Pap { tabs: Some(vec![TabStop { pos: 720, jc: 0, tlc: 0 }, TabStop { pos: 1440, jc: 1, tlc: 0 }]), ..Pap::default() };
        // sprmPChgTabsPapx: delete 720; add 2880 (right, dots)
        let (p, t, _) = pap_of(init.clone(), &[(0xc60d, &[1, 0xd0, 0x02, 1, 0x40, 0x0b, 0x0a])]);
        assert_eq!(p.tabs, Some(vec![TabStop { pos: 1440, jc: 1, tlc: 0 }, TabStop { pos: 2880, jc: 2, tlc: 1 }]));
        assert_eq!(p.tabs_cleared, Some(vec![720]));
        assert_eq!(t.keys(), &[PapKey::Tabs]);
        // sprmPChgTabs with a tolerance of 30: deleting 740 removes 720
        let (p, _, _) = pap_of(init, &[(0xc615, &[1, 0xe4, 0x02, 30, 0, 0])]);
        assert_eq!(p.tabs.unwrap().len(), 1);
        // adding a position that was cleared forgets the clear
        let (p, _, _) = pap_of(Pap::default(), &[(0xc60d, &[1, 0xd0, 0x02, 0]), (0xc60d, &[0, 1, 0xd0, 0x02, 0])]);
        assert_eq!(p.tabs_cleared, Some(vec![]));
    }

    #[test]
    fn the_tab_limit_is_a_listed_difference() {
        let mut g = Vec::new();
        for k in 0..3u16 {
            let mut body = vec![0u8, 200];
            for i in 0..200u16 {
                body.extend((k * 200 + i + 1).to_le_bytes());
            }
            body.extend(vec![0u8; 200]);
            // the long form of the operand: the size comes from the counts (cb = 255)
            g.extend(0xc615u16.to_le_bytes());
            g.push(255);
            let mut long = vec![0u8];
            long.push(200);
            for i in 0..200u16 {
                long.extend((k * 200 + i + 1).to_le_bytes());
            }
            long.extend(vec![0u8; 200]);
            g.extend(long);
            let _ = body;
        }
        let mut pap = Pap::default();
        let mut t = KeySet::new();
        for p in prls(&g) {
            apply_pap(&mut pap, &p, &mut t, &mut PapCtx::default());
        }
        let n = pap.tabs.unwrap().len();
        assert!(n > 0 && n <= MAX_TABS, "{n} tabs");
    }

    #[test]
    fn border_pruning() {
        let none = Brc { color: AUTO, width: 0, brc_type: 0, space: 0, shadow: false, frame: false, from_new: false };
        let single = Brc { brc_type: 1, ..none };
        let mut t = KeySet::new();
        t.add(PapKey::BrcTop);
        t.add(PapKey::BrcLeft);
        let pap = Pap { brc_top: Some(none), brc_left: Some(single), ..Pap::default() };
        prune_none_borders(&pap, &Pap::default(), &mut t);
        assert_eq!(t.keys(), &[PapKey::BrcLeft]);
    }

    fn tap_of(sprms: &[(u16, &[u8])]) -> Tap {
        let mut tap = Tap::new();
        for (s, o) in sprms {
            run(*s, o, |p| apply_tap(&mut tap, &p));
        }
        finish_tap(&mut tap);
        tap
    }

    /// A TDefTable operand: `n` cells of 1000 twips.
    fn tdef(n: u8) -> Vec<u8> {
        let mut b = vec![n];
        for i in 0..=i16::from(n) {
            b.extend((i * 1000).to_le_bytes());
        }
        for _ in 0..n {
            b.extend([0u8; 20]);
        }
        b
    }

    #[test]
    fn table_rows_cells_merge_insert_delete() {
        // run() puts the cb byte after the sprm; a TDefTable has a u16 "cb + 1" instead
        let mut g = vec![0x08, 0xd6];
        let body = tdef(3);
        g.extend((u16::try_from(body.len()).unwrap() + 1).to_le_bytes());
        g.extend(&body);
        let mut tap = Tap::new();
        for p in prls(&g) {
            apply_tap(&mut tap, &p);
        }
        assert_eq!((tap.centers.clone(), tap.tcs.len(), tap.insert_built), (vec![0, 1000, 2000, 3000], 3, Some(false)));
        run(0x5624, &[0, 2], |p| apply_tap(&mut tap, &p));
        assert_eq!((tap.tcs[0].horz_merge, tap.tcs[1].horz_merge, tap.tcs[2].horz_merge), (3, 1, 0));
        // insert one cell of 500 at 1: the following columns move right
        run(0x7621, &[1, 1, 0xf4, 0x01], |p| apply_tap(&mut tap, &p));
        assert_eq!((tap.tcs.len(), tap.centers.clone()), (4, vec![0, 1000, 1500, 2500, 3500]));
        // delete cells 1..3: the columns after them close up
        run(0x5622, &[1, 3], |p| apply_tap(&mut tap, &p));
        assert_eq!((tap.tcs.len(), tap.centers.clone()), (2, vec![0, 1000, 2000]));
        // the last cell cannot be deleted
        run(0x5622, &[0, 9], |p| apply_tap(&mut tap, &p));
        assert_eq!(tap.tcs.len(), 2);
        // sprmTDxaCol: cell 0 becomes 400 wide
        run(0x7623, &[0, 1, 0x90, 0x01], |p| apply_tap(&mut tap, &p));
        assert_eq!(tap.centers, vec![0, 400, 1400]);
    }

    #[test]
    fn rows_built_by_insert_alone_start_at_the_left_edge() {
        let tap = tap_of(&[(0x7621, &[0, 2, 0xe8, 0x03]), (0x9601, &[0xe8, 0x03]), (0x9602, &[0x6c, 0])]);
        assert_eq!(tap.insert_built, Some(true));
        assert_eq!(tap.centers, vec![892, 1892, 2892]);
    }

    #[test]
    fn the_table_work_budget_stops_structural_sprms() {
        let mut tap = Tap::new();
        for _ in 0..100_000 {
            run(0x7621, &[0, 255, 1, 0], |p| apply_tap(&mut tap, &p));
        }
        assert!(tap.tcs.len() <= MAX_TAP_CELLS);
        assert_eq!(tap.tcs.len() + 1, tap.centers.len());
    }

    #[test]
    fn section_properties_and_array_holes() {
        let mut sep = default_sep();
        run(0xf203, &[2, 0x10, 0x01], |p| apply_sep(&mut sep, &p));
        assert_eq!(sep.col_widths, vec![None, None, Some(0x110)]);
        run(0xb01f, &[0x00, 0x30], |p| apply_sep(&mut sep, &p));
        run(0x7044, &[1, 0, 1, 0], |p| apply_sep(&mut sep, &p));
        assert_eq!((sep.xa_page, sep.pgn_start), (0x3000, 0x10001));
        run(0x702b, &[0xff, 0xff, 0xff, 0xff], |p| apply_sep(&mut sep, &p));
        assert_eq!(sep.brc_top, None);
        assert!(sep.f_endnote && sep.bkc == 2);
    }

    #[test]
    fn default_chp_values() {
        let c = default_chp(3, 4, 5);
        assert_eq!(
            (c.hps, c.hps_bi, c.ftc_ascii, c.ftc_fe, c.ftc_other, c.ftc_bi, c.istd),
            (Some(20), Some(20), Some(3), Some(4), Some(5), Some(0), Some(10))
        );
        assert_eq!(c.lid, Some(0x400));
    }
}
