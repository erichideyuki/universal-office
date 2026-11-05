//! The mutable XML DOM of the Editing API (record `uo-docx-d3.md`, "DOM"): the port of the
//! System.Xml.Linq object model the C# `Editing/**` works on, with the XLinq semantics the
//! saved bytes depend on.
//!
//! - **One arena per [`super::DocxDocument`]**: every parsed part is a `Document` node of the
//!   arena; detached subtrees (copies being built, templates, samples, removed content) live in
//!   the same arena. Slots are never reused, so a [`NodeId`] never names another node; it
//!   carries the arena's stamp (a process-wide serial), so an id of another document is
//!   refused. Detached nodes stay alive until the document is dropped (the C# GC frees them at
//!   some point; nothing observable depends on when).
//! - **Names** are interned `(namespace, local)` pairs ([`Name`], [`Ns`]); the vocabulary of
//!   the API ([`n`]) is pre-seeded in a fixed order so every arena gives it the same ids.
//!   Namespace declarations are attributes, as in XLinq: `xmlns:p` is `(XMLNS, "p")`, the
//!   default `xmlns` is `(NONE, "xmlns")`. Prefixes are not stored: the serializer derives them
//!   from the declarations in scope (`super::write`).
//! - **Content**: an element with no child nodes is either "empty" (`<a/>`, XLinq `content ==
//!   null`, written `<a />`) or "open" (`<a></a>`, `content == ""`), which XLinq keeps.
//! - **Insertion clones**: adding a node that already has a parent (or the top of the target's
//!   own tree) inserts a deep copy and leaves the original in place (XLinq `AddNode`).
//! - **Dirty**: every mutation that raises an XLinq `Changed` event on an attached node marks
//!   the part (its `Document` node) dirty; mutations of detached nodes do not.
//!
//! Every walk is iterative (no recursion on deep trees).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use super::error::ApiError;

/// A namespace (interned URI; [`NS_NONE`] is "no namespace").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ns(pub(crate) u32);

/// An expanded name: namespace and local name (XLinq `XName`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name(pub(crate) u32);

/// A node of a document's arena. Ids of another document, or forged, are refused by every
/// public entry point ([`ApiError`] `InvalidOperation`); a slot is never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId {
    pub(crate) stamp: u32,
    pub(crate) index: u32,
}

static STAMPS: AtomicU32 = AtomicU32::new(1);

macro_rules! known_namespaces {
    ($($c:ident = $uri:literal,)*) => {
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[repr(u32)]
        enum KnownNs { $($c,)* }
        $(#[allow(missing_docs)] pub const $c: Ns = Ns(KnownNs::$c as u32);)*
        const KNOWN_NS: &[&str] = &[$($uri,)*];
    };
}

known_namespaces! {
    NS_NONE = "",
    NS_XML = "http://www.w3.org/XML/1998/namespace",
    NS_XMLNS = "http://www.w3.org/2000/xmlns/",
    NS_W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    NS_R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    NS_PKG_RELS = "http://schemas.openxmlformats.org/package/2006/relationships",
    NS_CT = "http://schemas.openxmlformats.org/package/2006/content-types",
    NS_WP = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
    NS_A = "http://schemas.openxmlformats.org/drawingml/2006/main",
    NS_PIC = "http://schemas.openxmlformats.org/drawingml/2006/picture",
    NS_MC = "http://schemas.openxmlformats.org/markup-compatibility/2006",
    NS_V = "urn:schemas-microsoft-com:vml",
    NS_O = "urn:schemas-microsoft-com:office:office",
    NS_W14 = "http://schemas.microsoft.com/office/word/2010/wordml",
    NS_WPS = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape",
    NS_M = "http://schemas.openxmlformats.org/officeDocument/2006/math",
    NS_CP = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties",
    NS_DC = "http://purl.org/dc/elements/1.1/",
    NS_DCTERMS = "http://purl.org/dc/terms/",
    NS_XSI = "http://www.w3.org/2001/XMLSchema-instance",
    NS_EXT_PROPS = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties",
    NS_CUSTOM_PROPS = "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties",
    NS_VT = "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes",
}

macro_rules! known_names {
    ($($c:ident = ($ns:ident, $local:literal),)*) => {
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[repr(u32)]
        enum KnownName { $($c,)* }
        /// The vocabulary of the Editing API, pre-interned in every arena (C# `W.*`, `Ns.* + …`).
        #[allow(non_upper_case_globals, missing_docs)]
        pub mod n {
            use super::{KnownName, Name};
            $(pub const $c: Name = Name(KnownName::$c as u32);)*
        }
        const KNOWN_NAMES: &[(Ns, &str)] = &[$(($ns, $local),)*];
    };
}

known_names! {
    // xmlns declarations and xml:*
    XMLNS = (NS_NONE, "xmlns"),
    xml_space = (NS_XML, "space"),
    // w:*
    document = (NS_W, "document"), body = (NS_W, "body"), p = (NS_W, "p"), pPr = (NS_W, "pPr"), r = (NS_W, "r"),
    rPr = (NS_W, "rPr"), t = (NS_W, "t"), tab = (NS_W, "tab"), br = (NS_W, "br"), cr = (NS_W, "cr"), sym = (NS_W, "sym"),
    noBreakHyphen = (NS_W, "noBreakHyphen"), softHyphen = (NS_W, "softHyphen"), ptab = (NS_W, "ptab"),
    delText = (NS_W, "delText"), instrText = (NS_W, "instrText"), delInstrText = (NS_W, "delInstrText"),
    fldChar = (NS_W, "fldChar"), fldCharType = (NS_W, "fldCharType"), fldSimple = (NS_W, "fldSimple"),
    instr = (NS_W, "instr"), hyperlink = (NS_W, "hyperlink"), anchor = (NS_W, "anchor"), history = (NS_W, "history"),
    sdt = (NS_W, "sdt"), sdtPr = (NS_W, "sdtPr"), sdtContent = (NS_W, "sdtContent"), alias = (NS_W, "alias"),
    tag = (NS_W, "tag"), lock = (NS_W, "lock"), id = (NS_W, "id"), val = (NS_W, "val"), name = (NS_W, "name"),
    type_ = (NS_W, "type"), smartTag = (NS_W, "smartTag"), customXml = (NS_W, "customXml"), dir = (NS_W, "dir"),
    bdo = (NS_W, "bdo"), ins = (NS_W, "ins"), del = (NS_W, "del"), moveFrom = (NS_W, "moveFrom"),
    moveTo = (NS_W, "moveTo"), tbl = (NS_W, "tbl"), tr = (NS_W, "tr"), trPr = (NS_W, "trPr"), tc = (NS_W, "tc"),
    tcPr = (NS_W, "tcPr"), sectPr = (NS_W, "sectPr"), headerReference = (NS_W, "headerReference"),
    footerReference = (NS_W, "footerReference"), pgSz = (NS_W, "pgSz"), pgMar = (NS_W, "pgMar"), cols = (NS_W, "cols"),
    col = (NS_W, "col"), space = (NS_W, "space"), equalWidth = (NS_W, "equalWidth"),
    pgNumType = (NS_W, "pgNumType"), titlePg = (NS_W, "titlePg"), sectPrChange = (NS_W, "sectPrChange"),
    pStyle = (NS_W, "pStyle"), rStyle = (NS_W, "rStyle"), tblStyle = (NS_W, "tblStyle"), numPr = (NS_W, "numPr"),
    ilvl = (NS_W, "ilvl"), numId = (NS_W, "numId"), ind = (NS_W, "ind"), jc = (NS_W, "jc"), keepNext = (NS_W, "keepNext"),
    b = (NS_W, "b"), i = (NS_W, "i"), u = (NS_W, "u"), strike = (NS_W, "strike"), color = (NS_W, "color"),
    shd = (NS_W, "shd"), vertAlign = (NS_W, "vertAlign"), rFonts = (NS_W, "rFonts"), drawing = (NS_W, "drawing"),
    pict = (NS_W, "pict"), object = (NS_W, "object"), control = (NS_W, "control"), altChunk = (NS_W, "altChunk"),
    subDoc = (NS_W, "subDoc"), txbxContent = (NS_W, "txbxContent"), hdr = (NS_W, "hdr"), ftr = (NS_W, "ftr"),
    footnotes = (NS_W, "footnotes"), endnotes = (NS_W, "endnotes"), footnote = (NS_W, "footnote"),
    endnote = (NS_W, "endnote"), footnoteReference = (NS_W, "footnoteReference"),
    endnoteReference = (NS_W, "endnoteReference"), comment = (NS_W, "comment"),
    commentRangeStart = (NS_W, "commentRangeStart"), commentRangeEnd = (NS_W, "commentRangeEnd"),
    commentReference = (NS_W, "commentReference"), bookmarkStart = (NS_W, "bookmarkStart"),
    bookmarkEnd = (NS_W, "bookmarkEnd"), styles = (NS_W, "styles"), style = (NS_W, "style"), styleId = (NS_W, "styleId"),
    default = (NS_W, "default"), basedOn = (NS_W, "basedOn"), next = (NS_W, "next"), link = (NS_W, "link"),
    numbering = (NS_W, "numbering"), abstractNum = (NS_W, "abstractNum"), abstractNumId = (NS_W, "abstractNumId"),
    num = (NS_W, "num"), nsid = (NS_W, "nsid"), lvl = (NS_W, "lvl"), start = (NS_W, "start"),
    lvlOverride = (NS_W, "lvlOverride"), startOverride = (NS_W, "startOverride"), numStyleLink = (NS_W, "numStyleLink"),
    styleLink = (NS_W, "styleLink"), lvlPicBulletId = (NS_W, "lvlPicBulletId"), numPicBullet = (NS_W, "numPicBullet"),
    numPicBulletId = (NS_W, "numPicBulletId"), numIdMacAtCleanup = (NS_W, "numIdMacAtCleanup"), fonts = (NS_W, "fonts"),
    font = (NS_W, "font"), settings = (NS_W, "settings"), embedTrueTypeFonts = (NS_W, "embedTrueTypeFonts"),
    left = (NS_W, "left"), w = (NS_W, "w"), h = (NS_W, "h"), top = (NS_W, "top"), bottom = (NS_W, "bottom"),
    right = (NS_W, "right"), gutter = (NS_W, "gutter"), lastRenderedPageBreak = (NS_W, "lastRenderedPageBreak"),
    proofErr = (NS_W, "proofErr"), permStart = (NS_W, "permStart"), permEnd = (NS_W, "permEnd"),
    w_ascii = (NS_W, "ascii"), w_hAnsi = (NS_W, "hAnsi"), w_eastAsia = (NS_W, "eastAsia"),
    w_cs = (NS_W, "cs"), w_fill = (NS_W, "fill"), themeColor = (NS_W, "themeColor"),
    uiPriority = (NS_W, "uiPriority"), unhideWhenUsed = (NS_W, "unhideWhenUsed"), rPrChange = (NS_W, "rPrChange"),
    pPrChange = (NS_W, "pPrChange"), tblPrChange = (NS_W, "tblPrChange"), tblPrExChange = (NS_W, "tblPrExChange"),
    trPrChange = (NS_W, "trPrChange"), tcPrChange = (NS_W, "tcPrChange"), tblGridChange = (NS_W, "tblGridChange"),
    numberingChange = (NS_W, "numberingChange"), moveFromRangeStart = (NS_W, "moveFromRangeStart"),
    moveFromRangeEnd = (NS_W, "moveFromRangeEnd"), moveToRangeStart = (NS_W, "moveToRangeStart"),
    moveToRangeEnd = (NS_W, "moveToRangeEnd"), customXmlInsRangeStart = (NS_W, "customXmlInsRangeStart"),
    customXmlInsRangeEnd = (NS_W, "customXmlInsRangeEnd"), customXmlDelRangeStart = (NS_W, "customXmlDelRangeStart"),
    customXmlDelRangeEnd = (NS_W, "customXmlDelRangeEnd"), customXmlMoveFromRangeStart = (NS_W, "customXmlMoveFromRangeStart"),
    customXmlMoveFromRangeEnd = (NS_W, "customXmlMoveFromRangeEnd"), customXmlMoveToRangeStart = (NS_W, "customXmlMoveToRangeStart"),
    customXmlMoveToRangeEnd = (NS_W, "customXmlMoveToRangeEnd"), cellIns = (NS_W, "cellIns"), cellDel = (NS_W, "cellDel"),
    cellMerge = (NS_W, "cellMerge"), trackRevisions = (NS_W, "trackRevisions"),
    documentProtection = (NS_W, "documentProtection"), writeProtection = (NS_W, "writeProtection"),
    attachedTemplate = (NS_W, "attachedTemplate"), dataSource = (NS_W, "dataSource"), headerSource = (NS_W, "headerSource"),
    mailMerge = (NS_W, "mailMerge"), frameset = (NS_W, "frameset"), separator = (NS_W, "separator"),
    continuationSeparator = (NS_W, "continuationSeparator"),
    // schema-order names of pPr, rPr, sectPr, settings
    spacing = (NS_W, "spacing"),
    keepLines = (NS_W, "keepLines"), pageBreakBefore = (NS_W, "pageBreakBefore"), framePr = (NS_W, "framePr"),
    widowControl = (NS_W, "widowControl"), suppressLineNumbers = (NS_W, "suppressLineNumbers"), pBdr = (NS_W, "pBdr"),
    tabs = (NS_W, "tabs"), suppressAutoHyphens = (NS_W, "suppressAutoHyphens"), kinsoku = (NS_W, "kinsoku"),
    wordWrap = (NS_W, "wordWrap"), overflowPunct = (NS_W, "overflowPunct"), topLinePunct = (NS_W, "topLinePunct"),
    autoSpaceDE = (NS_W, "autoSpaceDE"), autoSpaceDN = (NS_W, "autoSpaceDN"), bidi = (NS_W, "bidi"),
    adjustRightInd = (NS_W, "adjustRightInd"), snapToGrid = (NS_W, "snapToGrid"),
    contextualSpacing = (NS_W, "contextualSpacing"), mirrorIndents = (NS_W, "mirrorIndents"),
    suppressOverlap = (NS_W, "suppressOverlap"), textDirection = (NS_W, "textDirection"),
    textAlignment = (NS_W, "textAlignment"), textboxTightWrap = (NS_W, "textboxTightWrap"),
    outlineLvl = (NS_W, "outlineLvl"), divId = (NS_W, "divId"), cnfStyle = (NS_W, "cnfStyle"), bCs = (NS_W, "bCs"),
    iCs = (NS_W, "iCs"), caps = (NS_W, "caps"), smallCaps = (NS_W, "smallCaps"), dstrike = (NS_W, "dstrike"),
    outline = (NS_W, "outline"), shadow = (NS_W, "shadow"), emboss = (NS_W, "emboss"), imprint = (NS_W, "imprint"),
    noProof = (NS_W, "noProof"), vanish = (NS_W, "vanish"), webHidden = (NS_W, "webHidden"), kern = (NS_W, "kern"),
    position = (NS_W, "position"), sz = (NS_W, "sz"), szCs = (NS_W, "szCs"), highlight = (NS_W, "highlight"),
    effect = (NS_W, "effect"), bdr = (NS_W, "bdr"), fitText = (NS_W, "fitText"), rtl = (NS_W, "rtl"), em = (NS_W, "em"),
    lang = (NS_W, "lang"), eastAsianLayout = (NS_W, "eastAsianLayout"), specVanish = (NS_W, "specVanish"),
    oMath = (NS_W, "oMath"), footnotePr = (NS_W, "footnotePr"), endnotePr = (NS_W, "endnotePr"),
    paperSrc = (NS_W, "paperSrc"), pgBorders = (NS_W, "pgBorders"), lnNumType = (NS_W, "lnNumType"),
    formProt = (NS_W, "formProt"), vAlign = (NS_W, "vAlign"), noEndnote = (NS_W, "noEndnote"),
    rtlGutter = (NS_W, "rtlGutter"), docGrid = (NS_W, "docGrid"), printerSettings = (NS_W, "printerSettings"),
    view = (NS_W, "view"), zoom = (NS_W, "zoom"), removePersonalInformation = (NS_W, "removePersonalInformation"),
    removeDateAndTime = (NS_W, "removeDateAndTime"), doNotDisplayPageBoundaries = (NS_W, "doNotDisplayPageBoundaries"),
    displayBackgroundShape = (NS_W, "displayBackgroundShape"),
    printPostScriptOverText = (NS_W, "printPostScriptOverText"),
    printFractionalCharacterWidth = (NS_W, "printFractionalCharacterWidth"), printFormsData = (NS_W, "printFormsData"),
    embedSystemFonts = (NS_W, "embedSystemFonts"), saveSubsetFonts = (NS_W, "saveSubsetFonts"),
    saveFormsData = (NS_W, "saveFormsData"), mirrorMargins = (NS_W, "mirrorMargins"),
    alignBordersAndEdges = (NS_W, "alignBordersAndEdges"),
    bordersDoNotSurroundHeader = (NS_W, "bordersDoNotSurroundHeader"),
    bordersDoNotSurroundFooter = (NS_W, "bordersDoNotSurroundFooter"), gutterAtTop = (NS_W, "gutterAtTop"),
    hideSpellingErrors = (NS_W, "hideSpellingErrors"), hideGrammaticalErrors = (NS_W, "hideGrammaticalErrors"),
    activeWritingStyle = (NS_W, "activeWritingStyle"), proofState = (NS_W, "proofState"),
    formsDesign = (NS_W, "formsDesign"), linkStyles = (NS_W, "linkStyles"),
    stylePaneFormatFilter = (NS_W, "stylePaneFormatFilter"), stylePaneSortMethod = (NS_W, "stylePaneSortMethod"),
    documentType = (NS_W, "documentType"), revisionView = (NS_W, "revisionView"),
    doNotTrackMoves = (NS_W, "doNotTrackMoves"), doNotTrackFormatting = (NS_W, "doNotTrackFormatting"),
    // r:*
    r_id = (NS_R, "id"), r_embed = (NS_R, "embed"),
    // package relationships and content types
    Relationships = (NS_PKG_RELS, "Relationships"), Relationship = (NS_PKG_RELS, "Relationship"),
    Types = (NS_CT, "Types"), Default = (NS_CT, "Default"), Override = (NS_CT, "Override"),
    // drawing
    wp_inline = (NS_WP, "inline"), wp_anchor = (NS_WP, "anchor"), wp_extent = (NS_WP, "extent"),
    wp_effectExtent = (NS_WP, "effectExtent"), wp_docPr = (NS_WP, "docPr"), wp_cNvGraphicFramePr = (NS_WP, "cNvGraphicFramePr"),
    a_graphicFrameLocks = (NS_A, "graphicFrameLocks"), a_graphic = (NS_A, "graphic"), a_graphicData = (NS_A, "graphicData"),
    a_blip = (NS_A, "blip"), a_stretch = (NS_A, "stretch"), a_fillRect = (NS_A, "fillRect"), a_xfrm = (NS_A, "xfrm"),
    a_off = (NS_A, "off"), a_ext = (NS_A, "ext"), a_prstGeom = (NS_A, "prstGeom"), a_avLst = (NS_A, "avLst"),
    a_custGeom = (NS_A, "custGeom"), a_solidFill = (NS_A, "solidFill"), a_srgbClr = (NS_A, "srgbClr"), a_noFill = (NS_A, "noFill"),
    pic_pic = (NS_PIC, "pic"), pic_nvPicPr = (NS_PIC, "nvPicPr"), pic_cNvPr = (NS_PIC, "cNvPr"), pic_cNvPicPr = (NS_PIC, "cNvPicPr"),
    pic_blipFill = (NS_PIC, "blipFill"), pic_spPr = (NS_PIC, "spPr"),
    wps_wsp = (NS_WPS, "wsp"), wps_spPr = (NS_WPS, "spPr"), wps_cNvSpPr = (NS_WPS, "cNvSpPr"), wps_cNvCnPr = (NS_WPS, "cNvCnPr"),
    v_shape = (NS_V, "shape"), v_rect = (NS_V, "rect"), v_roundrect = (NS_V, "roundrect"), v_fill = (NS_V, "fill"),
    v_imagedata = (NS_V, "imagedata"), o_OLEObject = (NS_O, "OLEObject"), o_relid = (NS_O, "relid"),
    w14_paraId = (NS_W14, "paraId"), mc_Ignorable = (NS_MC, "Ignorable"), mc_Fallback = (NS_MC, "Fallback"),
    // document properties
    cp_coreProperties = (NS_CP, "coreProperties"), cp_keywords = (NS_CP, "keywords"), cp_lastModifiedBy = (NS_CP, "lastModifiedBy"),
    cp_category = (NS_CP, "category"), dc_title = (NS_DC, "title"), dc_subject = (NS_DC, "subject"), dc_creator = (NS_DC, "creator"),
    dc_description = (NS_DC, "description"), dcterms_modified = (NS_DCTERMS, "modified"), dcterms_created = (NS_DCTERMS, "created"),
    xsi_type = (NS_XSI, "type"), ext_Properties = (NS_EXT_PROPS, "Properties"), custom_Properties = (NS_CUSTOM_PROPS, "Properties"),
    custom_property = (NS_CUSTOM_PROPS, "property"), vt_lpwstr = (NS_VT, "lpwstr"),
    // attributes without namespace
    Id = (NS_NONE, "Id"), Type = (NS_NONE, "Type"), Target = (NS_NONE, "Target"), TargetMode = (NS_NONE, "TargetMode"),
    PartName = (NS_NONE, "PartName"), ContentType = (NS_NONE, "ContentType"), Extension = (NS_NONE, "Extension"),
    a_id = (NS_NONE, "id"), a_name = (NS_NONE, "name"), a_val = (NS_NONE, "val"), a_uri = (NS_NONE, "uri"),
    a_cx = (NS_NONE, "cx"), a_cy = (NS_NONE, "cy"), a_x = (NS_NONE, "x"), a_y = (NS_NONE, "y"), a_l = (NS_NONE, "l"),
    a_t = (NS_NONE, "t"), a_r = (NS_NONE, "r"), a_b = (NS_NONE, "b"), distT = (NS_NONE, "distT"), distB = (NS_NONE, "distB"),
    distL = (NS_NONE, "distL"), distR = (NS_NONE, "distR"), noChangeAspect = (NS_NONE, "noChangeAspect"),
    a_prst = (NS_NONE, "prst"), a_style = (NS_NONE, "style"), fillcolor = (NS_NONE, "fillcolor"), filled = (NS_NONE, "filled"),
    fmtid = (NS_NONE, "fmtid"), pid = (NS_NONE, "pid"),
    // namespace declarations of the parts the API creates
    xmlns_w = (NS_XMLNS, "w"), xmlns_r = (NS_XMLNS, "r"), xmlns_a = (NS_XMLNS, "a"), xmlns_pic = (NS_XMLNS, "pic"),
    xmlns_mc = (NS_XMLNS, "mc"), xmlns_cp = (NS_XMLNS, "cp"), xmlns_dc = (NS_XMLNS, "dc"), xmlns_dcterms = (NS_XMLNS, "dcterms"),
    xmlns_dcmitype = (NS_XMLNS, "dcmitype"), xmlns_xsi = (NS_XMLNS, "xsi"), xmlns_vt = (NS_XMLNS, "vt"),
}

/// An attribute: its expanded name and value (namespace declarations included, as in XLinq).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    pub name: Name,
    pub value: String,
}

/// The XML declaration of a part (XLinq `XDeclaration`; only `standalone` reaches the output).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Declaration {
    pub version: Option<String>,
    pub encoding: Option<String>,
    pub standalone: Option<String>,
}

/// What a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Document,
    Element,
    Text,
    CData,
    Comment,
    Pi,
}

#[derive(Debug, Clone)]
pub(crate) enum Data {
    Document { children: Vec<NodeId>, declaration: Option<Declaration>, dirty: bool },
    Element { name: Name, attrs: Vec<Attr>, children: Vec<NodeId>, open: bool },
    Text(String),
    Cdata(String),
    Comment(String),
    Pi { target: String, data: String },
}

#[derive(Debug, Clone)]
struct Slot {
    parent: u32,
    data: Data,
}

const NO_PARENT: u32 = u32::MAX;

/// The name table: namespaces and `(namespace, local)` pairs, the known ones first.
#[derive(Debug, Clone)]
struct Names {
    ns: Vec<Box<str>>,
    ns_index: HashMap<Box<str>, u32>,
    names: Vec<(Ns, Box<str>)>,
    by_local: HashMap<Box<str>, Vec<(Ns, Name)>>,
}

impl Names {
    fn new() -> Self {
        let mut t = Names { ns: Vec::new(), ns_index: HashMap::new(), names: Vec::new(), by_local: HashMap::new() };
        for uri in KNOWN_NS {
            t.intern_ns(uri);
        }
        for &(ns, local) in KNOWN_NAMES {
            t.push(ns, local);
        }
        t
    }

    fn intern_ns(&mut self, uri: &str) -> Ns {
        if let Some(&i) = self.ns_index.get(uri) {
            return Ns(i);
        }
        let i = u32::try_from(self.ns.len()).unwrap_or(u32::MAX);
        self.ns.push(uri.into());
        self.ns_index.insert(uri.into(), i);
        Ns(i)
    }

    fn push(&mut self, ns: Ns, local: &str) -> Name {
        let id = Name(u32::try_from(self.names.len()).unwrap_or(u32::MAX));
        self.names.push((ns, local.into()));
        self.by_local.entry(local.into()).or_default().push((ns, id));
        id
    }

    fn lookup(&self, ns: Ns, local: &str) -> Option<Name> {
        self.by_local.get(local)?.iter().find(|(n, _)| *n == ns).map(|&(_, id)| id)
    }

    fn intern(&mut self, ns: Ns, local: &str) -> Name {
        match self.lookup(ns, local) {
            Some(id) => id,
            None => self.push(ns, local),
        }
    }
}

/// The arena (see the module comment).
#[derive(Debug, Clone)]
pub struct Dom {
    stamp: u32,
    slots: Vec<Slot>,
    names: Names,
}

impl Default for Dom {
    fn default() -> Self {
        Dom::new()
    }
}

impl Dom {
    pub fn new() -> Self {
        Dom { stamp: STAMPS.fetch_add(1, Ordering::Relaxed), slots: Vec::new(), names: Names::new() }
    }

    // ─────────────────────────────── names ───────────────────────────────

    pub fn ns(&mut self, uri: &str) -> Ns {
        self.names.intern_ns(uri)
    }

    pub fn find_ns(&self, uri: &str) -> Option<Ns> {
        self.names.ns_index.get(uri).map(|&i| Ns(i))
    }

    pub fn ns_uri(&self, ns: Ns) -> &str {
        self.names.ns.get(ns.0 as usize).map_or("", |s| s)
    }

    pub fn name(&mut self, ns: Ns, local: &str) -> Name {
        self.names.intern(ns, local)
    }

    pub fn find_name(&self, ns: Ns, local: &str) -> Option<Name> {
        self.names.lookup(ns, local)
    }

    pub fn name_ns(&self, n: Name) -> Ns {
        self.names.names.get(n.0 as usize).map_or(NS_NONE, |(ns, _)| *ns)
    }

    pub fn local(&self, n: Name) -> &str {
        self.names.names.get(n.0 as usize).map_or("", |(_, l)| l)
    }

    /// Two names are the same expanded name (aliases of the vocabulary included).
    #[inline]
    pub fn same(&self, a: Name, b: Name) -> bool {
        a == b
    }

    /// The XLinq `IsNamespaceDeclaration` of an attribute name.
    pub fn is_ns_decl(&self, n: Name) -> bool {
        let ns = self.name_ns(n);
        ns == NS_XMLNS || (ns == NS_NONE && self.local(n) == "xmlns")
    }

    // ─────────────────────────────── ids ───────────────────────────────

    fn id(&self, index: usize) -> NodeId {
        NodeId { stamp: self.stamp, index: u32::try_from(index).unwrap_or(u32::MAX) }
    }

    /// The id belongs to this arena.
    pub fn check(&self, id: NodeId) -> Result<(), ApiError> {
        if id.stamp == self.stamp && (id.index as usize) < self.slots.len() {
            Ok(())
        } else {
            Err(ApiError::invalid_operation("the node id does not belong to this document"))
        }
    }

    pub(crate) fn data(&self, id: NodeId) -> Option<&Data> {
        if id.stamp != self.stamp {
            return None;
        }
        self.slots.get(id.index as usize).map(|s| &s.data)
    }

    fn data_mut(&mut self, id: NodeId) -> Option<&mut Data> {
        if id.stamp != self.stamp {
            return None;
        }
        self.slots.get_mut(id.index as usize).map(|s| &mut s.data)
    }

    fn alloc(&mut self, data: Data) -> NodeId {
        let id = self.id(self.slots.len());
        self.slots.push(Slot { parent: NO_PARENT, data });
        id
    }

    /// Number of slots (memory accounting).
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Bytes held by the arena (slots, children and attribute vectors, strings, names).
    pub fn heap_bytes(&self) -> usize {
        let mut total = self.slots.capacity() * std::mem::size_of::<Slot>();
        for s in &self.slots {
            total += match &s.data {
                Data::Document { children, .. } => children.capacity() * 8,
                Data::Element { attrs, children, .. } => {
                    children.capacity() * 8
                        + attrs.capacity() * std::mem::size_of::<Attr>()
                        + attrs.iter().map(|a| a.value.capacity()).sum::<usize>()
                }
                Data::Text(t) | Data::Cdata(t) | Data::Comment(t) => t.capacity(),
                Data::Pi { target, data } => target.capacity() + data.capacity(),
            };
        }
        total
            + self.names.names.iter().map(|(_, l)| l.len() + 24).sum::<usize>()
            + self.names.ns.iter().map(|s| s.len() + 16).sum::<usize>()
    }

    // ─────────────────────────────── creation ───────────────────────────────

    pub fn new_document(&mut self, declaration: Option<Declaration>) -> NodeId {
        self.alloc(Data::Document { children: Vec::new(), declaration, dirty: false })
    }

    pub fn new_element(&mut self, name: Name) -> NodeId {
        self.alloc(Data::Element { name, attrs: Vec::new(), children: Vec::new(), open: false })
    }

    /// An element with attributes (no duplicates expected: the callers build fixed shapes).
    pub fn new_element_with(&mut self, name: Name, attrs: &[(Name, &str)]) -> NodeId {
        let attrs = attrs.iter().map(|&(n, v)| Attr { name: n, value: v.to_owned() }).collect();
        self.alloc(Data::Element { name, attrs, children: Vec::new(), open: false })
    }

    pub fn new_text(&mut self, s: &str) -> NodeId {
        self.alloc(Data::Text(s.to_owned()))
    }

    pub(crate) fn new_raw(&mut self, data: Data) -> NodeId {
        self.alloc(data)
    }

    // ─────────────────────────────── reading ───────────────────────────────

    pub fn kind(&self, id: NodeId) -> Option<Kind> {
        Some(match self.data(id)? {
            Data::Document { .. } => Kind::Document,
            Data::Element { .. } => Kind::Element,
            Data::Text(_) => Kind::Text,
            Data::Cdata(_) => Kind::CData,
            Data::Comment(_) => Kind::Comment,
            Data::Pi { .. } => Kind::Pi,
        })
    }

    pub fn is_element(&self, id: NodeId) -> bool {
        matches!(self.data(id), Some(Data::Element { .. }))
    }

    /// The element's name (`None` for other nodes).
    pub fn name_of(&self, id: NodeId) -> Option<Name> {
        match self.data(id)? {
            Data::Element { name, .. } => Some(*name),
            _ => None,
        }
    }

    /// The element has this name.
    #[inline]
    pub fn is(&self, id: NodeId, name: Name) -> bool {
        self.name_of(id).is_some_and(|n| self.same(n, name))
    }

    /// The element's name is one of these.
    pub fn is_any(&self, id: NodeId, names: &[Name]) -> bool {
        self.name_of(id).is_some_and(|n| names.iter().any(|&m| self.same(n, m)))
    }

    pub fn local_of(&self, id: NodeId) -> &str {
        self.name_of(id).map_or("", |n| self.local(n))
    }

    pub fn ns_of(&self, id: NodeId) -> Ns {
        self.name_of(id).map_or(NS_NONE, |n| self.name_ns(n))
    }

    /// The node's container (element or document), XLinq `parent`.
    pub fn container(&self, id: NodeId) -> Option<NodeId> {
        if id.stamp != self.stamp {
            return None;
        }
        let p = self.slots.get(id.index as usize)?.parent;
        (p != NO_PARENT).then(|| self.id(p as usize))
    }

    /// The parent element (XLinq `XNode.Parent`: `None` under the document node).
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        let p = self.container(id)?;
        self.is_element(p).then_some(p)
    }

    /// The top of the node's tree (itself when detached).
    pub fn top(&self, id: NodeId) -> NodeId {
        let mut cur = id;
        while let Some(p) = self.container(cur) {
            cur = p;
        }
        cur
    }

    /// The document node the node is attached to, if any.
    pub fn document_of(&self, id: NodeId) -> Option<NodeId> {
        let t = self.top(id);
        matches!(self.data(t), Some(Data::Document { .. })).then_some(t)
    }

    /// The child nodes (empty for non-containers).
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        match self.data(id) {
            Some(Data::Element { children, .. } | Data::Document { children, .. }) => children,
            _ => &[],
        }
    }

    /// The element has no child nodes and is in the empty form (`<a/>`, XLinq `IsEmpty`).
    pub fn is_empty_form(&self, id: NodeId) -> bool {
        matches!(self.data(id), Some(Data::Element { children, open: false, .. }) if children.is_empty())
    }

    pub fn is_open_form(&self, id: NodeId) -> bool {
        matches!(self.data(id), Some(Data::Element { children, open: true, .. }) if children.is_empty())
    }

    /// The child elements (`Elements()`).
    pub fn elements(&self, id: NodeId) -> impl DoubleEndedIterator<Item = NodeId> + '_ {
        self.children(id).iter().copied().filter(|&c| self.is_element(c))
    }

    /// The child elements with a name (`Elements(name)`), as a snapshot.
    pub fn elements_named(&self, id: NodeId, name: Name) -> Vec<NodeId> {
        self.elements(id).filter(|&c| self.is(c, name)).collect()
    }

    /// The first child element with a name (`Element(name)`).
    pub fn element(&self, id: NodeId, name: Name) -> Option<NodeId> {
        self.elements(id).find(|&c| self.is(c, name))
    }

    /// The last child element (`Elements().LastOrDefault()`).
    pub fn last_element(&self, id: NodeId) -> Option<NodeId> {
        self.elements(id).next_back()
    }

    /// The descendant elements in document order (`Descendants()`), as a snapshot.
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<(NodeId, usize)> = vec![(id, 0)];
        while let Some((node, k)) = stack.pop() {
            let kids = self.children(node);
            let Some(&c) = kids.get(k) else { continue };
            stack.push((node, k + 1));
            if self.is_element(c) {
                out.push(c);
                stack.push((c, 0));
            }
        }
        out
    }

    /// `Descendants(name)`.
    pub fn descendants_named(&self, id: NodeId, name: Name) -> Vec<NodeId> {
        let mut v = self.descendants(id);
        v.retain(|&e| self.is(e, name));
        v
    }

    /// `DescendantsAndSelf()` (the node itself only when it is an element).
    pub fn descendants_and_self(&self, id: NodeId) -> Vec<NodeId> {
        let mut v = Vec::new();
        if self.is_element(id) {
            v.push(id);
        }
        v.extend(self.descendants(id));
        v
    }

    /// The first descendant with a name (`Descendants(name).FirstOrDefault()`), without a snapshot.
    pub fn first_descendant(&self, id: NodeId, pred: impl Fn(&Dom, NodeId) -> bool) -> Option<NodeId> {
        let mut stack: Vec<(NodeId, usize)> = vec![(id, 0)];
        while let Some((node, k)) = stack.pop() {
            let kids = self.children(node);
            let Some(&c) = kids.get(k) else { continue };
            stack.push((node, k + 1));
            if self.is_element(c) {
                if pred(self, c) {
                    return Some(c);
                }
                stack.push((c, 0));
            }
        }
        None
    }

    /// The ancestor elements, nearest first (`Ancestors()`).
    pub fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut cur = self.parent(id);
        while let Some(p) = cur {
            out.push(p);
            cur = self.parent(p);
        }
        out
    }

    /// The nearest ancestor with a name (`Ancestors(name).FirstOrDefault()`).
    pub fn ancestor(&self, id: NodeId, name: Name) -> Option<NodeId> {
        let mut cur = self.parent(id);
        while let Some(p) = cur {
            if self.is(p, name) {
                return Some(p);
            }
            cur = self.parent(p);
        }
        None
    }

    /// The furthest ancestor with a name (`Ancestors(name).LastOrDefault()`).
    pub fn last_ancestor(&self, id: NodeId, name: Name) -> Option<NodeId> {
        let mut found = None;
        let mut cur = self.parent(id);
        while let Some(p) = cur {
            if self.is(p, name) {
                found = Some(p);
            }
            cur = self.parent(p);
        }
        found
    }

    /// The ancestors-and-self chain (`AncestorsAndSelf()`), nearest first.
    pub fn ancestors_and_self(&self, id: NodeId) -> Vec<NodeId> {
        let mut v = Vec::new();
        if self.is_element(id) {
            v.push(id);
        }
        v.extend(self.ancestors(id));
        v
    }

    fn position(&self, id: NodeId) -> Option<(NodeId, usize)> {
        let p = self.container(id)?;
        let k = self.children(p).iter().position(|&c| c == id)?;
        Some((p, k))
    }

    /// The sibling elements after the node (`ElementsAfterSelf()`), as a snapshot.
    pub fn elements_after(&self, id: NodeId) -> Vec<NodeId> {
        let Some((p, k)) = self.position(id) else { return Vec::new() };
        self.children(p).iter().skip(k + 1).copied().filter(|&c| self.is_element(c)).collect()
    }

    /// The sibling elements after the node, lazily (`ElementsAfterSelf()` enumerated).
    pub fn following_elements(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let (kids, from): (&[NodeId], usize) = match self.position(id) {
            Some((p, k)) => (self.children(p), k + 1),
            None => (&[], 0),
        };
        kids.iter().skip(from).copied().filter(|&c| self.is_element(c))
    }

    /// The sibling elements before the node (`ElementsBeforeSelf()`), in document order.
    pub fn elements_before(&self, id: NodeId) -> Vec<NodeId> {
        let Some((p, k)) = self.position(id) else { return Vec::new() };
        self.children(p).iter().take(k).copied().filter(|&c| self.is_element(c)).collect()
    }

    /// The next sibling element (`ElementsAfterSelf().FirstOrDefault()`).
    pub fn next_element(&self, id: NodeId) -> Option<NodeId> {
        let (p, k) = self.position(id)?;
        self.children(p).iter().skip(k + 1).copied().find(|&c| self.is_element(c))
    }

    /// The attributes in order.
    pub fn attrs(&self, id: NodeId) -> &[Attr] {
        match self.data(id) {
            Some(Data::Element { attrs, .. }) => attrs,
            _ => &[],
        }
    }

    pub fn attr(&self, id: NodeId, name: Name) -> Option<&str> {
        self.attrs(id).iter().find(|a| self.same(a.name, name)).map(|a| a.value.as_str())
    }

    /// The text of a text, CDATA, comment or PI node.
    pub fn text(&self, id: NodeId) -> Option<&str> {
        match self.data(id)? {
            Data::Text(t) | Data::Cdata(t) | Data::Comment(t) => Some(t),
            Data::Pi { data, .. } => Some(data),
            _ => None,
        }
    }

    /// XLinq `XElement.Value`: the text and CDATA of the descendants, concatenated (for a text
    /// node, its text).
    pub fn value(&self, id: NodeId) -> String {
        match self.data(id) {
            Some(Data::Text(t) | Data::Cdata(t)) => return t.clone(),
            Some(Data::Element { .. }) => {}
            _ => return String::new(),
        }
        let mut out = String::new();
        let mut stack: Vec<(NodeId, usize)> = vec![(id, 0)];
        while let Some((node, k)) = stack.pop() {
            let Some(&c) = self.children(node).get(k) else { continue };
            stack.push((node, k + 1));
            match self.data(c) {
                Some(Data::Text(t) | Data::Cdata(t)) => out.push_str(t),
                Some(Data::Element { .. }) => stack.push((c, 0)),
                _ => {}
            }
        }
        out
    }

    /// The declaration of a document node.
    pub fn declaration(&self, doc: NodeId) -> Option<&Declaration> {
        match self.data(doc)? {
            Data::Document { declaration, .. } => declaration.as_ref(),
            _ => None,
        }
    }

    pub fn set_declaration(&mut self, doc: NodeId, decl: Option<Declaration>) {
        if let Some(Data::Document { declaration, .. }) = self.data_mut(doc) {
            *declaration = decl;
        }
    }

    /// The root element of a document node.
    pub fn root(&self, doc: NodeId) -> Option<NodeId> {
        self.elements(doc).next()
    }

    pub fn is_dirty(&self, doc: NodeId) -> bool {
        matches!(self.data(doc), Some(Data::Document { dirty: true, .. }))
    }

    pub fn set_dirty(&mut self, doc: NodeId, value: bool) {
        if let Some(Data::Document { dirty, .. }) = self.data_mut(doc) {
            *dirty = value;
        }
    }

    // ─────────────────────────────── mutation ───────────────────────────────

    /// An XLinq `Changed` event on `id`: marks its part dirty when it is attached.
    fn changed(&mut self, id: NodeId) {
        let t = self.top(id);
        self.set_dirty(t, true);
    }

    /// `SetAttributeValue(name, value)`: `None` removes; an existing attribute keeps its place.
    pub fn set_attr(&mut self, id: NodeId, name: Name, value: Option<&str>) {
        let pos = self.attrs(id).iter().position(|a| self.same(a.name, name));
        let Some(Data::Element { attrs, .. }) = self.data_mut(id) else { return };
        match (pos, value) {
            (Some(k), Some(v)) => {
                if let Some(a) = attrs.get_mut(k) {
                    a.value = v.to_owned();
                }
            }
            (Some(k), None) => {
                attrs.remove(k);
            }
            (None, Some(v)) => attrs.push(Attr { name, value: v.to_owned() }),
            (None, None) => return,
        }
        self.changed(id);
    }

    /// `Add(new XAttribute(name, value))`: a duplicate name is an error (XLinq
    /// `InvalidOperationException`).
    pub fn add_attr(&mut self, id: NodeId, name: Name, value: &str) -> Result<(), ApiError> {
        if self.attrs(id).iter().any(|a| self.same(a.name, name)) {
            return Err(ApiError::invalid_operation("Duplicate attribute."));
        }
        if let Some(Data::Element { attrs, .. }) = self.data_mut(id) {
            attrs.push(Attr { name, value: value.to_owned() });
            self.changed(id);
        }
        Ok(())
    }

    /// Removes attributes matching a predicate (`Attributes().Where(…).Remove()`).
    pub fn remove_attrs_where(&mut self, id: NodeId, pred: impl Fn(&Dom, &Attr) -> bool) {
        let keep: Vec<bool> = self.attrs(id).iter().map(|a| !pred(self, a)).collect();
        if keep.iter().all(|&k| k) {
            return;
        }
        if let Some(Data::Element { attrs, .. }) = self.data_mut(id) {
            let mut it = keep.iter();
            attrs.retain(|_| *it.next().unwrap_or(&true));
        }
        self.changed(id);
    }

    /// `XElement.Value = s` (remove the content, then add the text) or `XText.Value = s`.
    pub fn set_value(&mut self, id: NodeId, s: &str) {
        match self.data_mut(id) {
            Some(Data::Text(t) | Data::Cdata(t)) => {
                *t = s.to_owned();
                self.changed(id);
            }
            Some(Data::Element { .. }) => {
                self.detach_children(id);
                if let Some(Data::Element { children, open, .. }) = self.data_mut(id) {
                    children.clear();
                    *open = false;
                }
                self.add_text_inner(id, s);
                self.changed(id);
            }
            _ => {}
        }
    }

    fn detach_children(&mut self, id: NodeId) {
        let kids: Vec<NodeId> = self.children(id).to_vec();
        for k in kids {
            if let Some(s) = self.slots.get_mut(k.index as usize) {
                s.parent = NO_PARENT;
            }
        }
    }

    /// XLinq `AddString` at the end: "" on an empty-form element makes it open; text merges
    /// into a last text node (not CDATA).
    fn add_text_inner(&mut self, parent: NodeId, s: &str) -> bool {
        let last = self.children(parent).last().copied();
        let Some(Data::Element { children, open, .. }) = self.data_mut(parent) else { return false };
        if s.is_empty() {
            if children.is_empty() && !*open {
                *open = true;
                return true;
            }
            return false;
        }
        if let Some(l) = last
            && let Some(Data::Text(t)) = self.data_mut(l)
        {
            t.push_str(s);
            return true;
        }
        let t = self.alloc(Data::Text(s.to_owned()));
        if let Some(slot) = self.slots.get_mut(t.index as usize) {
            slot.parent = parent.index;
        }
        if let Some(Data::Element { children, .. }) = self.data_mut(parent) {
            children.push(t);
        }
        true
    }

    /// `Add(string)` at the end of an element.
    pub fn append_text(&mut self, parent: NodeId, s: &str) {
        if self.add_text_inner(parent, s) {
            self.changed(parent);
        }
    }

    /// The node to insert into `parent`: itself, or a deep copy when it already has a
    /// container or is the top of `parent`'s own tree (XLinq `AddNode`).
    fn insertable(&mut self, parent: NodeId, node: NodeId) -> Option<NodeId> {
        self.data(node)?;
        if matches!(self.data(node), Some(Data::Document { .. })) {
            return None;
        }
        if self.container(node).is_some() || self.top(parent) == node { Some(self.deep_clone(node)) } else { Some(node) }
    }

    /// Inserts nodes into `parent` at child index `at` (clamped), XLinq insertion semantics.
    /// Returns the inserted ids (copies where XLinq copies).
    fn insert_at(&mut self, parent: NodeId, at: usize, nodes: &[NodeId]) -> Vec<NodeId> {
        let mut ids = Vec::with_capacity(nodes.len());
        for &n in nodes {
            if let Some(x) = self.insertable(parent, n) {
                ids.push(x);
            }
        }
        if ids.is_empty() || !matches!(self.data(parent), Some(Data::Element { .. } | Data::Document { .. })) {
            return Vec::new();
        }
        for &x in &ids {
            if let Some(s) = self.slots.get_mut(x.index as usize) {
                s.parent = parent.index;
            }
        }
        match self.data_mut(parent) {
            Some(Data::Element { children, open, .. }) => {
                let at = at.min(children.len());
                children.splice(at..at, ids.iter().copied());
                *open = false;
            }
            Some(Data::Document { children, .. }) => {
                let at = at.min(children.len());
                children.splice(at..at, ids.iter().copied());
            }
            _ => {}
        }
        self.changed(parent);
        ids
    }

    /// `Add(node)` at the end.
    pub fn append(&mut self, parent: NodeId, node: NodeId) -> NodeId {
        let at = self.children(parent).len();
        self.insert_at(parent, at, &[node]).first().copied().unwrap_or(node)
    }

    /// `Add(nodes)` at the end.
    pub fn append_all(&mut self, parent: NodeId, nodes: &[NodeId]) -> Vec<NodeId> {
        let at = self.children(parent).len();
        self.insert_at(parent, at, nodes)
    }

    /// `AddFirst(nodes)`.
    pub fn prepend(&mut self, parent: NodeId, nodes: &[NodeId]) -> Vec<NodeId> {
        self.insert_at(parent, 0, nodes)
    }

    /// `AddBeforeSelf(nodes)`: an error without a container (XLinq "The parent is missing").
    pub fn add_before(&mut self, node: NodeId, nodes: &[NodeId]) -> Result<Vec<NodeId>, ApiError> {
        let (p, k) = self.position(node).ok_or_else(missing_parent)?;
        Ok(self.insert_at(p, k, nodes))
    }

    /// `AddAfterSelf(nodes)`.
    pub fn add_after(&mut self, node: NodeId, nodes: &[NodeId]) -> Result<Vec<NodeId>, ApiError> {
        let (p, k) = self.position(node).ok_or_else(missing_parent)?;
        Ok(self.insert_at(p, k + 1, nodes))
    }

    /// `Remove()`: an error without a container.
    pub fn remove(&mut self, node: NodeId) -> Result<(), ApiError> {
        let (p, k) = self.position(node).ok_or_else(missing_parent)?;
        if let Some(Data::Element { children, .. } | Data::Document { children, .. }) = self.data_mut(p) {
            children.remove(k);
        }
        if let Some(s) = self.slots.get_mut(node.index as usize) {
            s.parent = NO_PARENT;
        }
        self.changed(p);
        Ok(())
    }

    /// `Remove()` where the C# guards with `Parent is not null` or removes from a snapshot of
    /// attached nodes: a node without a container is left alone.
    pub fn remove_if_attached(&mut self, node: NodeId) {
        let _ = self.remove(node);
    }

    /// `RemoveNodes()`.
    pub fn remove_nodes(&mut self, parent: NodeId) {
        let had = !self.children(parent).is_empty() || self.is_open_form(parent);
        self.detach_children(parent);
        match self.data_mut(parent) {
            Some(Data::Element { children, open, .. }) => {
                children.clear();
                *open = false;
            }
            Some(Data::Document { children, .. }) => children.clear(),
            _ => return,
        }
        if had {
            self.changed(parent);
        }
    }

    /// `ReplaceWith(nodes)`: removes the node, then inserts the nodes where it was (copies of
    /// nodes that have a container, e.g. its own children).
    pub fn replace_with(&mut self, node: NodeId, nodes: &[NodeId]) -> Result<Vec<NodeId>, ApiError> {
        let (p, k) = self.position(node).ok_or_else(missing_parent)?;
        self.remove(node)?;
        Ok(self.insert_at(p, k, nodes))
    }

    /// Removes many nodes at once, each parent rebuilt once (C# `XmlTree.RemoveAll`).
    pub fn remove_all(&mut self, nodes: &[NodeId]) {
        let set: std::collections::HashSet<NodeId> = nodes.iter().copied().filter(|&n| self.container(n).is_some()).collect();
        let mut parents: Vec<NodeId> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for &n in nodes {
            if !set.contains(&n) {
                continue;
            }
            if let Some(p) = self.container(n)
                && seen.insert(p)
            {
                parents.push(p);
            }
        }
        for p in parents {
            let kept: Vec<NodeId> = self.children(p).iter().copied().filter(|c| !set.contains(c)).collect();
            // `parent.RemoveNodes(); parent.Add(kept)`
            self.remove_nodes(p);
            self.append_all(p, &kept);
        }
    }

    /// Replaces each element by its children, in batch per parent, deepest parents first (C#
    /// `XmlTree.Unwrap`).
    pub fn unwrap_all(&mut self, elements: &[NodeId]) {
        if elements.is_empty() {
            return;
        }
        let set: std::collections::HashSet<NodeId> = elements.iter().copied().filter(|&e| self.container(e).is_some()).collect();
        let mut parents: Vec<NodeId> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for &e in elements {
            if !set.contains(&e) {
                continue;
            }
            if let Some(p) = self.container(e)
                && seen.insert(p)
            {
                parents.push(p);
            }
        }
        // OrderByDescending(p => p.Ancestors().Count()): stable
        let mut keyed: Vec<(usize, NodeId)> = parents.into_iter().map(|p| (self.ancestors(p).len(), p)).collect();
        keyed.sort_by_key(|k| std::cmp::Reverse(k.0));
        for (_, p) in keyed {
            let nodes: Vec<NodeId> = self.children(p).to_vec();
            self.remove_nodes(p);
            let mut rebuilt = Vec::with_capacity(nodes.len());
            for n in nodes {
                if set.contains(&n) {
                    let kids: Vec<NodeId> = self.children(n).to_vec();
                    self.remove_nodes(n);
                    rebuilt.extend(kids);
                } else {
                    rebuilt.push(n);
                }
            }
            self.append_all(p, &rebuilt);
        }
    }

    /// A deep copy, detached (XLinq copy constructor / `XmlTree.DeepClone`: name, attributes,
    /// text, CDATA, comments, PIs and the empty/open form). Iterative.
    pub fn deep_clone(&mut self, id: NodeId) -> NodeId {
        let Some(data) = self.data(id).cloned() else { return id };
        let root = self.alloc(shallow(data));
        let mut stack = vec![(id, root)];
        while let Some((src, dst)) = stack.pop() {
            let kids: Vec<NodeId> = self.children(src).to_vec();
            let mut copies = Vec::with_capacity(kids.len());
            for k in kids {
                let Some(d) = self.data(k).cloned() else { continue };
                let c = self.alloc(shallow(d));
                if let Some(s) = self.slots.get_mut(c.index as usize) {
                    s.parent = dst.index;
                }
                copies.push(c);
                if self.is_element(k) {
                    stack.push((k, c));
                }
            }
            if let Some(Data::Element { children, .. } | Data::Document { children, .. }) = self.data_mut(dst) {
                *children = copies;
            }
        }
        root
    }

    /// A deep copy of a node of another arena into this one, detached (names re-interned).
    pub fn import(&mut self, src: &Dom, id: NodeId) -> Option<NodeId> {
        let data = src.data(id)?.clone();
        let d = self.translate(src, data);
        let root = self.alloc(d);
        let mut stack = vec![(id, root)];
        while let Some((s, dst)) = stack.pop() {
            let mut copies = Vec::new();
            for &k in src.children(s) {
                let Some(d) = src.data(k).cloned() else { continue };
                let d = self.translate(src, d);
                let c = self.alloc(d);
                if let Some(slot) = self.slots.get_mut(c.index as usize) {
                    slot.parent = dst.index;
                }
                copies.push(c);
                if src.is_element(k) {
                    stack.push((k, c));
                }
            }
            if let Some(Data::Element { children, .. } | Data::Document { children, .. }) = self.data_mut(dst) {
                *children = copies;
            }
        }
        Some(root)
    }

    fn translate_name(&mut self, src: &Dom, n: Name) -> Name {
        if (n.0 as usize) < KNOWN_NAMES.len() {
            return n;
        }
        let uri = src.ns_uri(src.name_ns(n)).to_owned();
        let ns = self.ns(&uri);
        let local = src.local(n).to_owned();
        self.name(ns, &local)
    }

    fn translate(&mut self, src: &Dom, d: Data) -> Data {
        match shallow(d) {
            Data::Element { name, attrs, open, .. } => {
                let name = self.translate_name(src, name);
                let attrs = attrs.into_iter().map(|a| Attr { name: self.translate_name(src, a.name), value: a.value }).collect();
                Data::Element { name, attrs, children: Vec::new(), open }
            }
            other => other,
        }
    }
}

/// The node without its children (the copy's children are filled by the caller).
fn shallow(d: Data) -> Data {
    match d {
        Data::Element { name, attrs, children, open } => {
            Data::Element { name, attrs, open: open && children.is_empty(), children: Vec::new() }
        }
        Data::Document { declaration, .. } => Data::Document { children: Vec::new(), declaration, dirty: false },
        other => other,
    }
}

fn missing_parent() -> ApiError {
    ApiError::invalid_operation("The parent is missing.")
}
