//! The DOP ([MS-DOC] 2.7: `DopBase` + `Dop95/97/2000`…) to `settings.xml` (`settings.ts`). The Word
//! opens a `.doc` in compatibility mode and, saved as `.docx`, writes `compatibilityMode` = 11
//! (Word 2003) and a `w:compat` with the options of `Copts60`/`Copts80`/`Copts`, reproduced here
//! bit by bit, in the order of the schema (`CT_Compat`).

use super::bytes::ByteReader;
use super::fonts::DocFont;
use super::xmlw::{AttrVal, Attrs, XML_DECL, attrs, el, ns};

/// Where a compat flag's bit comes from.
#[derive(Clone, Copy)]
enum Src {
    C60,
    C80,
    C,
    C2,
    Fe,
}

/// A bit of origin and the `w:compat` element it makes; `inv`: the element is the opposite of the bit.
struct CompatFlag {
    el: &'static str,
    src: Src,
    bit: u32,
    inv: bool,
}

const fn cf(el: &'static str, src: Src, bit: u32) -> CompatFlag {
    CompatFlag { el, src, bit, inv: false }
}

const fn cfi(el: &'static str, src: Src, bit: u32) -> CompatFlag {
    CompatFlag { el, src, bit, inv: true }
}

/// In the order of `CT_Compat` (ECMA-376 part 1 17.15.3 / part 4 14.8.3).
const COMPAT: &[CompatFlag] = &[
    cf("useSingleBorderforContiguousCells", Src::C60, 8),
    cf("wpJustification", Src::C80, 14),
    cf("noTabHangInd", Src::C60, 0),
    cf("noLeading", Src::C80, 3),
    cfi("spaceForUL", Src::C80, 4),
    cf("noColumnBalance", Src::C60, 5),
    cfi("balanceSingleByteDoubleByteWidth", Src::C60, 15),
    cf("noExtraLineSpacing", Src::C80, 11),
    cfi("doNotLeaveBackslashAlone", Src::C60, 12),
    cfi("ulTrailSpace", Src::C60, 14),
    cfi("doNotExpandShiftReturn", Src::C60, 13),
    cf("spacingInWholePoints", Src::C80, 1),
    cf("lineWrapLikeWord6", Src::C80, 9),
    cf("printBodyTextBeforeHeader", Src::C80, 2),
    cf("printColBlack", Src::C60, 4),
    cf("wpSpaceWidth", Src::C80, 13),
    cf("showBreaksInFrames", Src::C60, 10),
    cf("subFontBySize", Src::C80, 8),
    cf("suppressBottomSpacing", Src::C80, 12),
    cf("suppressTopSpacing", Src::C60, 7),
    cf("suppressSpacingAtTopOfPage", Src::C80, 0),
    cf("suppressTopSpacingWP", Src::C80, 6),
    cf("suppressSpBfAfterPgBrk", Src::C60, 2),
    cf("swapBordersFacingPages", Src::C60, 11),
    cf("convMailMergeEsc", Src::C60, 6),
    cf("truncateFontHeightsLikeWP6", Src::C80, 7),
    cf("mwSmallCaps", Src::C80, 5),
    cf("usePrinterMetrics", Src::C80, 15),
    cf("doNotSuppressParagraphBorders", Src::C80, 10),
    cf("wrapTrailSpaces", Src::C60, 3),
    cf("footnoteLayoutLikeWW8", Src::C, 1),
    cf("shapeLayoutLikeWW8", Src::C, 0),
    cf("alignTablesRowByRow", Src::C, 6),
    cf("forgetLastTabAlignment", Src::C, 4),
    cfi("adjustLineHeightInTable", Src::C, 3),
    cf("autoSpaceLikeWord95", Src::C, 5),
    cf("noSpaceRaiseLower", Src::C60, 1),
    cf("doNotUseHTMLParagraphAutoSpacing", Src::C, 2),
    cf("layoutRawTableWidth", Src::C, 7),
    cf("layoutTableRowsApart", Src::C, 8),
    cf("useWord97LineBreakRules", Src::C, 9),
    cf("doNotBreakWrappedTables", Src::C, 10),
    cf("doNotSnapToGridInCell", Src::C, 11),
    cf("selectFldWithFirstOrLastChar", Src::C, 12),
    cf("applyBreakingRules", Src::C, 13),
    cf("doNotWrapTextWithPunct", Src::C, 14),
    cf("doNotUseEastAsianBreakRules", Src::C, 15),
    cf("useWord2002TableStyleRules", Src::C, 16),
    cf("growAutofit", Src::C, 17),
    cf("useFELayout", Src::Fe, 0),
    cf("useNormalStyleForList", Src::C, 18),
    cf("doNotUseIndentAsNumberingTabStop", Src::C, 19),
    cf("useAltKinsokuLineBreakRules", Src::C, 20),
    cf("allowSpaceOfSameStyleInTable", Src::C, 21),
    cf("doNotSuppressIndentation", Src::C, 22),
    cf("doNotAutofitConstrainedTables", Src::C, 23),
    cf("autofitToFirstFixedWidthCell", Src::C, 24),
    cf("underlineTabInNumList", Src::C, 25),
    cf("displayHangulFixedWidth", Src::C, 26),
    cf("splitPgBreakAndParaMark", Src::C, 27),
    cf("doNotVertAlignCellWithSp", Src::C, 28),
    cf("doNotBreakConstrainedForcedTable", Src::C, 29),
    cf("doNotVertAlignInTxbx", Src::C, 30),
    cf("useAnsiKerningPairs", Src::C, 31),
    cf("cachedColBalance", Src::C2, 0),
];

/// The drawing grid (`DOGRID`, Dop97 @400).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    pub xa: u16,
    pub ya: u16,
    pub dxa: u16,
    pub dya: u16,
    pub dy_display: u16,
    pub dx_display: u16,
    pub follow_margins: bool,
}

/// The document properties (`Dop`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dop {
    pub facing_pages: bool,
    pub mirror_margins: bool,
    pub dxa_tab: u16,
    pub dxa_hot_z: u16,
    pub c_consec_hyp_lim: u16,
    pub auto_hyphen: bool,
    pub hyph_capitals: bool,
    pub rev_marking: bool,
    pub fpc: u8,
    pub rnc_ftn: u16,
    pub n_ftn: u16,
    pub rnc_edn: u16,
    pub n_edn: u16,
    pub epc: u16,
    pub zoom: u16,
    pub copts60: u16,
    pub copts80hi: u16,
    pub copts: Option<u32>,
    pub copts2: Option<u32>,
    /// protection: `DopBase.fProtEnabled` / `fLockAtn` / `fLockRev` and Dop2003
    pub prot_forms: bool,
    pub lock_atn: bool,
    pub lock_rev: bool,
    pub lock_atn_read_only: bool,
    pub style_lock: bool,
    pub enforce_doc_prot: bool,
    pub doc_prot_cur: u16,
    pub print_form_data: bool,
    pub save_form_data: bool,
    pub shade_form_data: bool,
    pub include_subdocs_in_stats: bool,
    pub snap_border: bool,
    pub include_header: bool,
    pub include_footer: bool,
    pub kerning_punct: bool,
    pub justification: u16,
    pub grid: Option<Grid>,
}

/// `readDop`: the DOP of the table stream (`bytes`) of a document of version `n_fib`.
#[must_use]
pub fn read_dop(bytes: &[u8], n_fib: u16) -> Dop {
    let r = ByteReader::new(bytes);
    let len = bytes.len();
    let b0 = r.u8(0);
    let f32 = r.u32(4);
    let bit = |n: u32| f32 & (1 << n) != 0;
    let mut dop = Dop {
        facing_pages: b0 & 1 != 0,
        fpc: (b0 >> 5) & 3,
        rnc_ftn: r.u16(2) & 3,
        n_ftn: r.u16(2) >> 2,
        mirror_margins: bit(21),
        auto_hyphen: bit(12),
        hyph_capitals: bit(11),
        rev_marking: bit(15),
        copts60: r.u16(8),
        dxa_tab: if r.u16(10) == 0 { 720 } else { r.u16(10) },
        dxa_hot_z: r.u16(14),
        c_consec_hyp_lim: r.u16(16),
        rnc_edn: r.u16(52) & 3,
        n_edn: r.u16(52) >> 2,
        epc: r.u16(54) & 3,
        zoom: (r.u16(82) >> 3) & 0x1ff,
        copts80hi: if len >= 88 { r.u16(86) } else { 0 },
        copts: None,
        copts2: None,
        prot_forms: bit(25),
        lock_atn: bit(20),
        lock_rev: bit(30),
        // Dop2003 (after the 594 bytes of the Dop2002): flags A..M at 594, O..T at 598
        lock_atn_read_only: len >= 600 && r.u16(594) & 1 != 0,
        style_lock: len >= 600 && r.u16(594) & 2 != 0,
        enforce_doc_prot: len >= 600 && r.u16(598) & 8 != 0,
        doc_prot_cur: if len >= 600 { (r.u16(598) >> 4) & 7 } else { 7 },
        print_form_data: r.u16(54) & 0x400 != 0,
        save_form_data: r.u16(54) & 0x800 != 0,
        shade_form_data: r.u16(54) & 0x1000 != 0,
        include_subdocs_in_stats: r.u16(54) & 0x8000 != 0,
        snap_border: len >= 412 && r.u16(410) & 0x800 != 0,
        include_header: len < 412 || r.u16(410) & 0x1000 != 0,
        include_footer: len < 412 || r.u16(410) & 0x2000 != 0,
        kerning_punct: len >= 92 && r.u16(90) & 1 != 0,
        justification: if len >= 92 { (r.u16(90) >> 1) & 3 } else { 0 },
        grid: None,
    };
    if len >= 410 {
        let g = r.u16(408);
        dop.grid = Some(Grid {
            xa: r.u16(400),
            ya: r.u16(402),
            dxa: r.u16(404),
            dya: r.u16(406),
            dy_display: g & 0x7f,
            dx_display: (g >> 8) & 0x7f,
            follow_margins: g & 0x8000 != 0,
        });
    }
    // Dop2000+: Copts (32 bytes) at 508 = copts80 repeated + flags (512) + fCachedColBalance (516)
    // (the Dop2000 flags before it take 32 bits: 504-507)
    if len >= 520 {
        dop.copts = Some(r.u32(512));
        dop.copts2 = Some(r.u32(516));
    }
    // a Word 2000/97 document: the Word turns on the emulations of the later features (bits K..f of Copts,
    // but fApplyBreakingRules), measured in the Word (nFib 0xD9)
    if n_fib <= 0xd9 {
        dop.copts = Some(dop.copts.unwrap_or(0) | 0xffff_dc00);
    }
    // the Word always writes w:cachedColBalance when it saves a .doc as .docx
    dop.copts2 = Some(dop.copts2.unwrap_or(0) | 1);
    dop
}

/// The document protection (measured against the Word's `.docx`): the old flags of `DopBase`
/// (`fProtEnabled`/`fLockAtn`/`fLockRev`) say the protection is ENFORCED; without them
/// `fEnforceDocProt` + `iDocProtCur` of the Dop2003 give the type with `enforcement="0"`. The
/// password hash cannot be converted (the Word generates another one).
fn protection_xml(dop: &Dop) -> String {
    let mut edit: Option<&'static str> = None;
    let mut enforced = true;
    if dop.lock_rev {
        edit = Some("trackedChanges");
    } else if dop.lock_atn {
        edit = Some(if dop.lock_atn_read_only { "readOnly" } else { "comments" });
    } else if dop.prot_forms {
        edit = Some("forms");
    } else if dop.enforce_doc_prot {
        edit = ["trackedChanges", "comments", "forms", "readOnly"].get(usize::from(dop.doc_prot_cur)).copied();
        enforced = false;
    }
    if edit.is_none() && !dop.style_lock {
        return String::new();
    }
    el(
        "w:documentProtection",
        &Attrs::of(&[
            ("w:edit", edit.map(AttrVal::Str)),
            ("w:formatting", dop.style_lock.then_some(AttrVal::Str("1"))),
            ("w:enforcement", Some(AttrVal::Str(if edit.is_some() && enforced { "1" } else { "0" }))),
        ]),
        "",
    )
}

fn compat_xml(dop: &Dop, fe_layout: bool) -> String {
    let mut x = String::new();
    for f in COMPAT {
        let mut on = match f.src {
            Src::Fe => fe_layout,
            Src::C60 => u32::from(dop.copts60) & (1 << f.bit) != 0,
            Src::C80 => u32::from(dop.copts80hi) & (1 << f.bit) != 0,
            Src::C => {
                let Some(c) = dop.copts else { continue };
                (c >> f.bit) & 1 != 0
            }
            Src::C2 => {
                let Some(c) = dop.copts2 else { continue };
                (c >> f.bit) & 1 != 0
            }
        };
        if f.inv {
            on = !on;
        }
        if on {
            x += &format!("<w:{}/>", f.el);
        }
    }
    // the .doc opens in the compatibility mode of Word 2003
    x += &el(
        "w:compatSetting",
        &Attrs::of(&[
            ("w:name", Some(AttrVal::Str("compatibilityMode"))),
            ("w:uri", Some(AttrVal::Str("http://schemas.microsoft.com/office/word"))),
            ("w:val", Some(AttrVal::Int(11))),
        ]),
        "",
    );
    format!("<w:compat>{x}</w:compat>")
}

fn int(v: u16) -> Option<AttrVal<'static>> {
    Some(AttrVal::Int(i64::from(v)))
}

fn val(v: Option<AttrVal<'_>>) -> Attrs<'_> {
    Attrs::of(&[("w:val", v)])
}

/// `buildSettings`: the `settings.xml` of the document. `dop` is the DOP stream of the table stream
/// (`DocReader.dop`), `n_fib` its FIB version, `fonts` the font table and `ftc_fe` the default East
/// Asian font of the style sheet; `footnotes`/`endnotes` say whether the document has notes.
#[must_use]
pub fn build_settings(dop_bytes: &[u8], n_fib: u16, fonts: &[DocFont], ftc_fe: u16, footnotes: bool, endnotes: bool) -> String {
    let dop = read_dop(dop_bytes, n_fib);
    // useFELayout: the Word turns it on when the default East Asian font has a CJK charset
    let fe = fonts.get(usize::from(ftc_fe));
    let fe_layout = fe.is_some_and(|f| [128, 129, 130, 134, 136].contains(&f.charset));
    let mut x = String::new();
    if (10..=500).contains(&dop.zoom) {
        x += &el("w:zoom", &Attrs::of(&[("w:percent", int(dop.zoom))]), "");
    }
    for (on, tag) in [
        (dop.print_form_data, "<w:printFormsData/>"),
        (dop.save_form_data, "<w:saveFormsData/>"),
        (dop.mirror_margins, "<w:mirrorMargins/>"),
        (dop.snap_border, "<w:alignBordersAndEdges/>"),
        (!dop.include_header, "<w:bordersDoNotSurroundHeader/>"),
        (!dop.include_footer, "<w:bordersDoNotSurroundFooter/>"),
        (dop.rev_marking, "<w:trackRevisions/>"),
    ] {
        if on {
            x += tag;
        }
    }
    x += &protection_xml(&dop);
    x += &el("w:defaultTabStop", &val(int(dop.dxa_tab)), "");
    if dop.auto_hyphen {
        x += "<w:autoHyphenation/>";
    }
    if dop.c_consec_hyp_lim != 0 {
        x += &el("w:consecutiveHyphenLimit", &val(int(dop.c_consec_hyp_lim)), "");
    }
    // the Word omits the default zone (360 = 0.25")
    if dop.dxa_hot_z != 0 && dop.dxa_hot_z != 360 {
        x += &el("w:hyphenationZone", &val(int(dop.dxa_hot_z)), "");
    }
    if !dop.hyph_capitals {
        x += "<w:doNotHyphenateCaps/>";
    }
    if dop.facing_pages {
        x += "<w:evenAndOddHeaders/>";
    }
    // the drawing grid: the Word omits the defaults (180 twips, shown every 1, origin 1701/1984)
    if let Some(g) = dop.grid {
        if g.dxa != 180 {
            x += &el("w:drawingGridHorizontalSpacing", &val(int(g.dxa)), "");
        }
        if g.dya != 180 {
            x += &el("w:drawingGridVerticalSpacing", &val(int(g.dya)), "");
        }
        if g.dx_display != 1 {
            x += &el("w:displayHorizontalDrawingGridEvery", &val(int(g.dx_display)), "");
        }
        if g.dy_display != 1 {
            x += &el("w:displayVerticalDrawingGridEvery", &val(int(g.dy_display)), "");
        }
        if !g.follow_margins {
            x += "<w:doNotUseMarginsForDrawingGridOrigin/>";
            if g.xa != 1701 {
                x += &el("w:drawingGridHorizontalOrigin", &val(int(g.xa)), "");
            }
            if g.ya != 1984 {
                x += &el("w:drawingGridVerticalOrigin", &val(int(g.ya)), "");
            }
        }
    }
    if !dop.shade_form_data {
        x += "<w:doNotShadeFormData/>";
    }
    if !dop.kerning_punct {
        x += "<w:noPunctuationKerning/>";
    }
    let csc = ["doNotCompress", "compressPunctuation", "compressPunctuationAndJapaneseKana"]
        .get(usize::from(dop.justification))
        .copied()
        .unwrap_or("doNotCompress");
    x += &el("w:characterSpacingControl", &val(Some(AttrVal::Str(csc))), "");
    if footnotes {
        x += "<w:footnotePr><w:footnote w:id=\"-1\"/><w:footnote w:id=\"0\"/></w:footnotePr>";
    }
    if endnotes {
        x += "<w:endnotePr><w:endnote w:id=\"-1\"/><w:endnote w:id=\"0\"/></w:endnotePr>";
    }
    x += &compat_xml(&dop, fe_layout);
    if !dop.include_subdocs_in_stats {
        x += "<w:doNotIncludeSubdocsInStats/>";
    }
    let a = attrs(&Attrs::of(&[
        ("xmlns:w", Some(AttrVal::Str(ns::W))),
        ("xmlns:r", Some(AttrVal::Str(ns::R))),
        ("xmlns:m", Some(AttrVal::Str(ns::M))),
    ]));
    format!("{XML_DECL}<w:settings{a}>{x}</w:settings>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Utf16;

    #[test]
    fn an_empty_dop_reads_as_the_defaults() {
        let d = read_dop(&[], 0x112);
        assert_eq!((d.dxa_tab, d.zoom, d.include_header, d.include_footer, d.doc_prot_cur, d.justification), (720, 0, true, true, 7, 0));
        assert_eq!((d.copts, d.copts2, d.grid), (None, Some(1), None));
        // a Word 97/2000 document turns on the later emulations
        assert_eq!(read_dop(&[], 0xd9).copts, Some(0xffff_dc00));
        assert_eq!(read_dop(&[], 0xda).copts, None);
    }

    #[test]
    fn settings_for_an_empty_dop() {
        let x = build_settings(&[], 0x112, &[], 0, false, false);
        assert!(x.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<w:settings xmlns:w="));
        assert!(x.contains("<w:defaultTabStop w:val=\"720\"/><w:doNotHyphenateCaps/><w:doNotShadeFormData/><w:noPunctuationKerning/>"));
        assert!(x.contains("<w:characterSpacingControl w:val=\"doNotCompress\"/><w:compat>"));
        assert!(x.contains("<w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"11\"/></w:compat><w:doNotIncludeSubdocsInStats/>"));
        // without a DopBase the Copts are absent and so are the elements from them; cachedColBalance always comes
        assert!(!x.contains("<w:footnotePr>"));
        let y = build_settings(&[], 0x112, &[], 0, true, true);
        assert!(y.contains("<w:footnotePr><w:footnote w:id=\"-1\"/><w:footnote w:id=\"0\"/></w:footnotePr><w:endnotePr>"));
    }

    #[test]
    fn protection_zoom_and_the_east_asian_layout() {
        let mut d = vec![0u8; 100];
        d[82..84].copy_from_slice(&(150u16 << 3).to_le_bytes());
        // fLockRev (bit 30 of the u32 at 4)
        d[4..8].copy_from_slice(&(1u32 << 30).to_le_bytes());
        let font = |charset| DocFont {
            name: Utf16::from("F"),
            alt_name: None,
            prq: 0,
            true_type: false,
            ff: 0,
            weight: 0,
            charset,
            panose: Vec::new(),
            sig: [0; 6],
        };
        let x = build_settings(&d, 0x112, &[font(134)], 0, false, false);
        assert!(x.contains("<w:zoom w:percent=\"150\"/>"));
        assert!(x.contains("<w:trackRevisions/>") || !read_dop(&d, 0x112).rev_marking);
        assert!(x.contains("<w:documentProtection w:edit=\"trackedChanges\" w:enforcement=\"1\"/>"));
        assert!(x.contains("<w:useFELayout/>"));
        let y = build_settings(&d, 0x112, &[font(0)], 0, false, false);
        assert!(!y.contains("useFELayout"));
    }
}
