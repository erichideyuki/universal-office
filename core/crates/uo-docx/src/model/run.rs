//! Runs and the objects they carry (docx-core `document.ts`): editing props, inline images,
//! textboxes, shapes, groups, notes, fields, math, form fields, bookmarks. Read by frontiers R4a
//! (props), R4b (objects) and R4c (runs, fields).

use std::sync::Arc;

use crate::ids::{DrawingId, SourceKey, TxbxSrc};
use crate::js::{JsNumber, JsObject, JsValue, JsonValue, JsonWriter, Str};
use crate::js_object;
use crate::text::Utf16;

use super::drawing::{DrawingAnchor, Ltrb, ShapeStyle};
use super::effects::{OpenTypeProps, TextEffects};
use super::media::MediaRef;
use super::para::Paragraph;
use super::picture::PictureFormat;
use super::props::{RPr, RevisionInfo};
use super::story::Story;

js_object! {
    /// A theme colour reference (`ThemeColorAttrs`: `w:themeColor` + tint/shade in hex).
    pub struct ThemeColorAttrs {
        req theme_color: Str = "themeColor",
        opt tint: Str = "tint",
        opt shade: Str = "shade",
    }
}

js_object! {
    /// `w:rPrChange` in editing form (`FormatChange`: `{ ...revInfo, old }`).
    pub struct FormatChange {
        req author: Str = "author",
        opt date: Str = "date",
        opt date_utc: Str = "dateUtc",
        opt r#move: bool = "move",
        req old: Box<RunProps> = "old",
    }
    hidden {
        /// the symbol keys `old` carries (it is a spread of the run's props when the session
        /// records the change: `runFormatting(run.props)`); see [`Run::rpr_link`]
        pub old_link: Option<Box<RprLink>>
    }
}

js_object! {
    /// The editing props of a run (`RunProps`). In the TS `bold`, `italic`, `underline`,
    /// `strike`, `allCaps` and `smallCaps` are required; they are `Option` here so that
    /// `Partial<RunProps>` (resolved styles, patches) is the same type — the reader always sets
    /// them (`DEFAULT_RUN_PROPS`). Declared in the assignment order of `runPropsFromRPr`.
    pub struct RunProps {
        opt bold: bool = "bold",
        opt italic: bool = "italic",
        opt underline: bool = "underline",
        opt strike: bool = "strike",
        opt all_caps: bool = "allCaps",
        opt small_caps: bool = "smallCaps",
        /// `w:u` style when not single
        opt underline_style: Str = "underlineStyle",
        opt underline_color: Str = "underlineColor",
        opt underline_theme: ThemeColorAttrs = "underlineTheme",
        opt color_theme: ThemeColorAttrs = "colorTheme",
        opt hidden: bool = "hidden",
        opt scale_pct: JsNumber = "scalePct",
        opt position_half_pt: JsNumber = "positionHalfPt",
        opt kern_half_pt: JsNumber = "kernHalfPt",
        opt double_strike: bool = "doubleStrike",
        /// superscript | subscript
        opt vert_align: Str = "vertAlign",
        opt sz_half_pt: JsNumber = "szHalfPt",
        opt color: Str = "color",
        opt highlight: Str = "highlight",
        opt font_ascii: Str = "fontAscii",
        opt char_spacing_twips: JsNumber = "charSpacingTwips",
        opt char_style: Str = "charStyle",
        opt text_fx: Box<TextEffects> = "textFx",
        opt open_type: Box<OpenTypeProps> = "openType",
        opt fmt: Box<FormatChange> = "fmt",
        opt link: Str = "link",
        opt link_tip: Str = "linkTip",
        opt no_proof: bool = "noProof",
        opt web_hidden: bool = "webHidden",
        opt is_math: bool = "isMath",
        opt comment_ids: Vec<JsNumber> = "commentIds",
        opt ins: Box<RevisionInfo> = "ins",
        opt del: Box<RevisionInfo> = "del",
        opt src: SourceKey = "src",
    }
}

js_object! {
    /// Colour adjustments of an image (`ImageEffects`).
    pub struct ImageEffects {
        opt gray: bool = "gray",
        opt gain: JsNumber = "gain",
        opt bright: JsNumber = "bright",
        opt floor: bool = "floor",
        opt bi_level: JsNumber = "biLevel",
    }
}

js_object! {
    /// `InlineImage.ole` (an embedded OLE object).
    pub struct OleData {
        req prog_id: Str = "progId",
        req data: MediaRef = "data",
        req ext: Str = "ext",
    }
}

js_object! {
    /// `InlineImage.layer` (`a14:imgLayer` original).
    pub struct ImageLayer {
        req data: MediaRef = "data",
        req ext: Str = "ext",
    }
}

js_object! {
    /// An image run's picture (`InlineImage`).
    pub struct InlineImage {
        req data: MediaRef = "data",
        req ext: Str = "ext",
        opt svg: MediaRef = "svg",
        req width_twips: JsNumber = "widthTwips",
        req height_twips: JsNumber = "heightTwips",
        opt floating: bool = "floating",
        opt offset_x_twips: JsNumber = "offsetXTwips",
        opt offset_y_twips: JsNumber = "offsetYTwips",
        opt rel_h: Str = "relH",
        opt rel_v: Str = "relV",
        opt behind_doc: bool = "behindDoc",
        opt raw_drawing_xml: Str = "rawDrawingXml",
        opt anchor: Box<DrawingAnchor> = "anchor",
        opt inline_effect: Ltrb = "inlineEffect",
        opt crop: Ltrb = "crop",
        opt style: Box<ShapeStyle> = "style",
        opt vml: bool = "vml",
        opt effects: ImageEffects = "effects",
        opt ole: Box<OleData> = "ole",
        opt edited: bool = "edited",
        opt layer: ImageLayer = "layer",
        opt format: Box<PictureFormat> = "format",
    }
    hidden {
        /// the drawing object (identity: PDF image, selection, uo-render RQ3)
        pub drawing: DrawingId
    }
}

js_object! {
    /// `DrawingShape.line` (VML line end points relative to the corner, twips).
    pub struct VmlLine {
        req x1: JsNumber = "x1",
        req y1: JsNumber = "y1",
        req x2: JsNumber = "x2",
        req y2: JsNumber = "y2",
    }
}

js_object! {
    /// A shape without text (`DrawingShape`).
    pub struct DrawingShape {
        req style: ShapeStyle = "style",
        req width_twips: JsNumber = "widthTwips",
        req height_twips: JsNumber = "heightTwips",
        opt floating: bool = "floating",
        opt anchor: Box<DrawingAnchor> = "anchor",
        opt inline_effect: Ltrb = "inlineEffect",
        opt line: VmlLine = "line",
        opt hr: bool = "hr",
        opt hr_pct: JsNumber = "hrPct",
        opt hr_no_shade: bool = "hrNoShade",
        opt hr_align: Str = "hrAlign",
        opt raw_vml: Str = "rawVml",
        opt hidden: bool = "hidden",
        opt raw_drawing_xml: Str = "rawDrawingXml",
        opt edited: bool = "edited",
    }
    hidden {
        pub drawing: DrawingId
    }
}

js_object! {
    /// `DrawingTextbox.geom` (a preset that is not a rectangle).
    pub struct TextboxGeom {
        req prst: Str = "prst",
        opt adj: crate::js::JsRecord<JsNumber> = "adj",
    }
}

js_object! {
    /// `DrawingTextbox.link` (a later box of a chain: story id and position).
    pub struct TextboxLink {
        req id: JsNumber = "id",
        req seq: JsNumber = "seq",
    }
}

/// How a textbox's TS views relate (A2: the `paras` view and the `blocks` view of one content).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextboxView {
    /// `blocks` present; `paras` = the paragraphs of the blocks, cells included (`textboxBlocks`)
    #[default]
    Synced,
    /// `blocks` present without any paragraph; `paras` = one fresh empty paragraph
    /// (`[{ runs: [], props: {} }]`) that is not in the blocks
    Phantom,
    /// no `blocks` key (read without a block reader: comments); `paras` = the story's paragraphs
    ParasOnly,
    /// `blocks` present; `paras` = the top-level paragraphs of the blocks only, cells excluded
    /// (`blocks.flatMap(b => b.type === 'paragraph' ? [b.paragraph] : [])`: the ODF text frame,
    /// odf/index.ts:594). The same objects as [`TextboxView::Synced`] when the blocks hold no
    /// cell paragraph; otherwise the TS canon calls the views "split"
    TopLevel,
}

js_object! {
    custom_value:
    /// A textbox (`DrawingTextbox`): its content is one [`Story`] (A2); the TS `paras` and
    /// `blocks` are views of it ([`TextboxView`]).
    pub struct DrawingTextbox {
        req width_twips: JsNumber = "widthTwips",
        req height_twips: JsNumber = "heightTwips",
        opt floating: bool = "floating",
        opt offset_x_twips: JsNumber = "offsetXTwips",
        opt offset_y_twips: JsNumber = "offsetYTwips",
        opt rel_h: Str = "relH",
        opt rel_v: Str = "relV",
        opt behind_doc: bool = "behindDoc",
        opt auto_fit: bool = "autoFit",
        opt no_wrap: bool = "noWrap",
        opt insets: Ltrb = "insets",
        opt raw_drawing_xml: Str = "rawDrawingXml",
        opt anchor: Box<DrawingAnchor> = "anchor",
        opt shape: Box<ShapeStyle> = "shape",
        opt line_inset: super::drawing::LineInset = "lineInset",
        opt geom: TextboxGeom = "geom",
        /// t | ctr | b
        opt v_anchor: Str = "vAnchor",
        opt rot: JsNumber = "rot",
        opt word_art: Str = "wordArt",
        opt vml: bool = "vml",
        opt story: JsNumber = "story",
        opt link: TextboxLink = "link",
        opt vml_id: Str = "vmlId",
        opt vml_next: Str = "vmlNext",
        opt vert: Str = "vert",
        opt upright: bool = "upright",
        opt hidden: bool = "hidden",
        /// RRGGBB or `@scheme|mods` (a spec string of `srgbOf`)
        opt font_color: super::color::DrawingColor = "fontColor",
        opt edited: bool = "edited",
    }
    hidden {
        /// the content, stored once
        pub content: Story,
        /// how `paras` relates to `blocks`
        pub view: TextboxView,
        pub drawing: DrawingId,
        /// the source groups of the content (`SourceStore.txbx`)
        pub txbx_src: Option<TxbxSrc>,
        /// the TS `paras` and `blocks` are equal copies, not the same objects: what the JSON round
        /// trip of an undo step leaves (D2a, U4; the TS relinks only the body tables)
        pub copies: ViewsCopied,
        /// wave 7 (B): the flat editing view of the box in edit while it holds a table (the TS edits
        /// `tb.paras` and reconciles `tb.blocks`); uo-docx `session/ops/hf_flat.rs`
        pub flat: super::section::EditSlot<Arc<super::section::HfFlat>>
    }
}

/// Whether a textbox's two TS views are copies after an undo (see [`DrawingTextbox::copies`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ViewsCopied(pub bool);

impl JsValue for DrawingTextbox {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        let content = &self.content;
        match self.view {
            TextboxView::ParasOnly => {
                crate::js::object_canon_with(self, &[("paras", &|w| content.canon_paragraphs(w))], w);
            }
            // the top-level paragraphs are a subsequence of the walked ones: equal counts = the
            // same objects ("synced"), else both views are written
            TextboxView::TopLevel
                if content.flat_paragraphs().len() != content.paras.len() || (self.copies.0 && content.has_paragraph()) =>
            {
                crate::js::object_canon_with(
                    self,
                    &[("blocks", &|w| content.canon(w)), ("paras", &|w| content.canon_paragraphs(w)), ("@views", &|w| w.str("split"))],
                    w,
                );
            }
            // equal copies (after an undo): both views, "split" (a view without paragraphs is the
            // same empty list either way)
            TextboxView::Synced if self.copies.0 && !content.flat_paragraphs().is_empty() => {
                let flat = content.flat_paragraphs();
                crate::js::object_canon_with(
                    self,
                    &[("blocks", &|w| content.canon(w)), ("paras", &|w| flat.canon(w)), ("@views", &|w| w.str("split"))],
                    w,
                );
            }
            v => {
                let views = if v == TextboxView::Phantom { "phantom" } else { "synced" };
                crate::js::object_canon_with(self, &[("blocks", &|w| content.canon(w)), ("@views", &|w| w.str(views))], w);
            }
        }
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        if w.is_signature() {
            // D1b signature text: the content once (`paras` for a box without blocks, else the
            // block view), with the view kind
            let view = match self.view {
                TextboxView::Synced => "synced",
                TextboxView::Phantom => "phantom",
                TextboxView::ParasOnly => "paras",
                TextboxView::TopLevel => "toplevel",
            };
            let blocks = super::story::BlocksJson(&self.content);
            crate::js::object_json_with(self, &[], &[("@view", &view), ("blocks", &blocks)], w);
            return true;
        }
        let phantom = Paragraph::default();
        let paras: Vec<&Paragraph> = match self.view {
            TextboxView::Phantom => vec![&phantom],
            TextboxView::TopLevel => self.content.paras.iter().collect(),
            _ => self.content.flat_paragraphs(),
        };
        let blocks = super::story::BlocksJson(&self.content);
        if self.view == TextboxView::ParasOnly {
            crate::js::object_json_with(self, &[("paras", &paras)], &[], w);
        } else {
            crate::js::object_json_with(self, &[("paras", &paras)], &[("blocks", &blocks)], w);
        }
        true
    }
}

impl JsValue for &Paragraph {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        (**self).canon(w);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        (**self).json(w)
    }
}

js_object! {
    /// A note reference mark (`NoteRef`).
    pub struct NoteRef {
        /// footnote | endnote
        req kind: Str = "kind",
        req id: JsNumber = "id",
        opt custom: bool = "custom",
    }
}

js_object! {
    /// An atomic inline registered by a plugin (`CustomInline`).
    pub struct CustomInline {
        req r#type: Str = "type",
        req data: JsonValue = "data",
        req fallback_text: Utf16 = "fallbackText",
    }
}

js_object! {
    /// One node of a display equation (`MathNode`, a permissive object for the union
    /// `{ t: 'r', text, sty?, rpr? } | { t: 'f', num, den, type? } | { t: 'br', rpr? }`).
    pub struct MathNode {
        req t: Str = "t",
        opt text: Utf16 = "text",
        opt num: Vec<MathNode> = "num",
        opt den: Vec<MathNode> = "den",
        opt sty: Str = "sty",
        opt r#type: Str = "type",
        opt rpr: Box<RPr> = "rpr",
    }
}

js_object! {
    /// A display equation (`MathZone`, `m:oMathPara`).
    pub struct MathZone {
        opt jc: Str = "jc",
        req nodes: Vec<MathNode> = "nodes",
    }
}

js_object! {
    /// `w:ptab` (`Run['ptab']`).
    pub struct Ptab {
        req alignment: Str = "alignment",
        req relative_to: Str = "relativeTo",
        opt leader: Str = "leader",
    }
}

js_object! {
    /// A legacy form field (`Run['formField']`).
    pub struct FormField {
        /// checkBox | textInput | dropDown
        req r#type: Str = "type",
        opt checked: bool = "checked",
        opt size_half_pt: JsNumber = "sizeHalfPt",
        opt size_auto: bool = "sizeAuto",
        opt entries: Vec<Utf16> = "entries",
        opt selected: JsNumber = "selected",
    }
}

js_object! {
    /// A bookmark mark (`BookmarkMark`).
    pub struct BookmarkMark {
        req id: JsNumber = "id",
        req name: Str = "name",
        opt end: bool = "end",
    }
}

js_object! {
    /// One item of a drawing group (`DrawingGroup.items[i]`).
    pub struct GroupItem {
        req x: JsNumber = "x",
        req y: JsNumber = "y",
        req w: JsNumber = "w",
        req h: JsNumber = "h",
        req run: Run = "run",
    }
}

js_object! {
    /// A drawing group (`DrawingGroup`: `wpg:wgp`, `v:group`, a canvas).
    pub struct DrawingGroup {
        req width_twips: JsNumber = "widthTwips",
        req height_twips: JsNumber = "heightTwips",
        opt rot: JsNumber = "rot",
        opt flip_h: bool = "flipH",
        opt flip_v: bool = "flipV",
        opt floating: bool = "floating",
        opt anchor: Box<DrawingAnchor> = "anchor",
        req items: Vec<GroupItem> = "items",
        opt hidden: bool = "hidden",
        opt raw_drawing_xml: Str = "rawDrawingXml",
        opt edited: bool = "edited",
    }
    hidden {
        pub drawing: DrawingId
    }
}

js_object! {
    /// The rare fields of a run (A2 `RunExt`): flattened into the run's keys in the digest and in
    /// JSON.
    pub struct RunExt {
        opt image: Box<InlineImage> = "image",
        opt textbox: Box<DrawingTextbox> = "textbox",
        opt note_ref: NoteRef = "noteRef",
        /// field instruction (`w:fldSimple w:instr`)
        opt field: Str = "field",
        /// field code with nested fields in braces
        opt field_code: Str = "fieldCode",
        opt custom: Box<CustomInline> = "custom",
        opt field_result: Vec<Run> = "fieldResult",
        opt shape: Box<DrawingShape> = "shape",
        opt ptab: Ptab = "ptab",
        opt sym_font: Str = "symFont",
        opt group: Box<DrawingGroup> = "group",
        opt in_toc: bool = "inToc",
        opt form_field: Box<FormField> = "formField",
        opt math: Box<MathZone> = "math",
        opt bookmark: BookmarkMark = "bookmark",
        opt field_begin: Str = "fieldBegin",
        opt field_end: bool = "fieldEnd",
        opt field_open: bool = "fieldOpen",
    }
    hidden {
        /// WordCraft study (fields, B22): the direct `w:rPr` of the field's first `w:instrText` run — Word takes the
        /// language of the `\*` words from it (measured), not from the begin or the result runs. Reader-only: outside
        /// the TS shape, the JSON, the digest and the equality ([`InstrRPr`])
        pub instr_rpr: InstrRPr
    }
}

/// The run properties of a field's instruction (see [`RunExt::instr_rpr`]): a fact of the reader that the TS model does
/// not hold, so it never makes two runs different (`PartialEq` is always true) and the undo round trip keeps it.
#[derive(Debug, Clone, Default)]
pub struct InstrRPr(pub Option<Arc<RPr>>);

impl PartialEq for InstrRPr {
    fn eq(&self, _: &InstrRPr) -> bool {
        true
    }
}

js_object! {
    custom_value:
    /// A run (`Run`): text in UTF-16, shared props, the direct `w:rPr`, and the rare fields in
    /// [`RunExt`].
    pub struct Run {
        /// U+FFFC for an object run
        req text: Utf16 = "text",
        req props: Arc<RunProps> = "props",
        /// the full direct `w:rPr` (tri-state), for the layout
        opt rpr: Arc<RPr> = "rpr",
        opt rpr_edited: bool = "rprEdited",
    }
    hidden {
        pub ext: Option<Box<RunExt>>,
        /// the TS symbol keys `RPR_REF`/`RPR_EDITED` that `universal-write.ts` keeps on the props
        /// object (`syncRunRPr`): invisible to equality, JSON and the digest, copied wherever the
        /// TS copies the props by spread, dropped by the undo normalisation (P2: on the run, not on
        /// the props, so that props stay internable; D2a)
        pub rpr_link: Option<Box<RprLink>>
    }
}

/// The link from a run's props to the `w:rPr` it was read with (see [`Run::rpr_link`]).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RprLink {
    /// `props[RPR_REF]`
    pub rpr: Option<Arc<RPr>>,
    /// `props[RPR_EDITED]`
    pub edited: bool,
}

impl Run {
    /// The rare fields (empty when none).
    pub fn ext(&self) -> Option<&RunExt> {
        self.ext.as_deref()
    }

    /// The rare fields for writing (created on first use).
    pub fn ext_mut(&mut self) -> &mut RunExt {
        self.ext.get_or_insert_with(Box::default)
    }
}

impl JsValue for Run {
    #[cfg(feature = "canon")]
    fn canon(&self, w: &mut crate::canon::CanonWriter<'_>) {
        let Some(ext) = self.ext() else {
            crate::js::object_canon(self, w);
            return;
        };
        let slots = ext.slots_in_order();
        let fns: Vec<crate::canon::Entry<'_>> = slots
            .into_iter()
            .map(|(k, s)| -> crate::canon::Entry<'_> {
                match s {
                    crate::js::Slot::Val(v) => (k, Box::new(move |w| v.canon(w))),
                    _ => (k, Box::new(|w| w.undefined())),
                }
            })
            .collect();
        let refs: Vec<crate::canon::EntryRef<'_>> = fns.iter().map(|(k, f)| (*k, &**f)).collect();
        crate::js::object_canon_with(self, &refs, w);
    }
    fn json(&self, w: &mut JsonWriter<'_>) -> bool {
        let after: Vec<(&str, &dyn JsValue)> = match self.ext() {
            Some(ext) => ext
                .slots_in_order()
                .into_iter()
                .filter_map(|(k, s)| if let crate::js::Slot::Val(v) = s { Some((k, v)) } else { None })
                .collect(),
            None => Vec::new(),
        };
        crate::js::object_json_with(self, &[], &after, w);
        true
    }
}
