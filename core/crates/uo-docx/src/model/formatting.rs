//! The document's formatting parts (docx-core `ooxml/parts.ts`, `styles.ts`): style sheet,
//! numbering, theme, settings, font table, and the resolved styles of `readStyles`. Read by
//! frontier R2. Dictionaries are [`JsRecord`]s (JS object order: integer-like `styleId`s first,
//! uo-docx-layout requirement).

use crate::js::{JsMap, JsNumber, JsRecord, Str};
use crate::js_object;

use super::drawing::{ShapeShadow, ShapeStyle};
use super::media::ImageData;
use super::para::ParaProps;
use super::props::{PPr, RPr, TblPr, TcPr, TrPr};
use super::run::RunProps;

js_object! {
    /// `w:tblStylePr` of a table style (`TblStylePart`).
    pub struct TblStylePart {
        opt p_pr: Box<PPr> = "pPr",
        opt r_pr: Box<RPr> = "rPr",
        opt tbl_pr: Box<TblPr> = "tblPr",
        opt tr_pr: Box<TrPr> = "trPr",
        opt tc_pr: Box<TcPr> = "tcPr",
    }
}

js_object! {
    /// One `w:style` (`StyleDef`).
    pub struct StyleDef {
        req style_id: Str = "styleId",
        /// paragraph | character | table | numbering
        req r#type: Str = "type",
        opt name: Str = "name",
        opt based_on: Str = "basedOn",
        opt next: Str = "next",
        opt link: Str = "link",
        opt is_default: bool = "isDefault",
        opt p_pr: Box<PPr> = "pPr",
        opt r_pr: Box<RPr> = "rPr",
        opt tbl_pr: Box<TblPr> = "tblPr",
        opt tr_pr: Box<TrPr> = "trPr",
        opt tc_pr: Box<TcPr> = "tcPr",
        opt tbl_style_pr: JsRecord<TblStylePart> = "tblStylePr",
    }
    hidden {
        /// WordCraft study (fields, B21): `w:aliases` (a STYLEREF may name the style by any of them, measured); a fact
        /// of the reader outside the TS shape, the JSON and the equality ([`StyleAliases`])
        pub aliases: StyleAliases
    }
}

/// The `w:aliases` of a style (see [`StyleDef::aliases`]): never makes two styles different.
#[derive(Debug, Clone, Default)]
pub struct StyleAliases(pub Option<Box<str>>);

impl PartialEq for StyleAliases {
    fn eq(&self, _: &StyleAliases) -> bool {
        true
    }
}

js_object! {
    /// `w:docDefaults` (`StyleSheet.docDefaults`).
    pub struct DocDefaults {
        opt r_pr: Box<RPr> = "rPr",
        opt p_pr: Box<PPr> = "pPr",
    }
}

js_object! {
    /// The default style id per type (`StyleSheet.defaults`).
    pub struct StyleDefaults {
        opt paragraph: Str = "paragraph",
        opt character: Str = "character",
        opt table: Str = "table",
        opt numbering: Str = "numbering",
    }
}

js_object! {
    /// `styles.xml` (`StyleSheet`).
    pub struct StyleSheet {
        req doc_defaults: DocDefaults = "docDefaults",
        req styles: JsRecord<StyleDef> = "styles",
        req defaults: StyleDefaults = "defaults",
    }
}

js_object! {
    /// One numbering level (`NumLevel`).
    pub struct NumLevel {
        req ilvl: JsNumber = "ilvl",
        opt start: JsNumber = "start",
        opt num_fmt: Str = "numFmt",
        opt num_fmt_custom: Str = "numFmtCustom",
        opt lvl_restart: JsNumber = "lvlRestart",
        opt p_style: Str = "pStyle",
        opt is_lgl: bool = "isLgl",
        opt suff: Str = "suff",
        opt lvl_text: Str = "lvlText",
        opt lvl_text_null: bool = "lvlTextNull",
        opt lvl_pic_bullet_id: JsNumber = "lvlPicBulletId",
        opt lvl_jc: Str = "lvlJc",
        opt p_pr: Box<PPr> = "pPr",
        opt r_pr: Box<RPr> = "rPr",
    }
}

js_object! {
    /// `w:abstractNum` (`AbstractNum`).
    pub struct AbstractNum {
        req abstract_num_id: JsNumber = "abstractNumId",
        opt multi_level_type: Str = "multiLevelType",
        opt style_link: Str = "styleLink",
        opt num_style_link: Str = "numStyleLink",
        req levels: JsRecord<NumLevel> = "levels",
    }
}

js_object! {
    /// `w:lvlOverride` (`NumInstance.overrides[ilvl]`).
    pub struct NumOverride {
        opt start_override: JsNumber = "startOverride",
        opt lvl: Box<NumLevel> = "lvl",
    }
}

js_object! {
    /// `w:num` (`NumInstance`).
    pub struct NumInstance {
        req num_id: JsNumber = "numId",
        req abstract_num_id: JsNumber = "abstractNumId",
        req overrides: JsRecord<NumOverride> = "overrides",
    }
}

js_object! {
    /// `w:numPicBullet` (`PicBullet`).
    pub struct PicBullet {
        req id: JsNumber = "id",
        opt rid: Str = "rid",
        opt width_pt: JsNumber = "widthPt",
        opt height_pt: JsNumber = "heightPt",
        opt image: ImageData = "image",
    }
}

js_object! {
    /// `numbering.xml` (`Numbering`).
    pub struct Numbering {
        req abstract_nums: JsRecord<AbstractNum> = "abstractNums",
        req nums: JsRecord<NumInstance> = "nums",
        req pic_bullets: JsRecord<PicBullet> = "picBullets",
    }
}

js_object! {
    /// A theme font set (`ThemeFontSet`); `scripts` first (the TS literal `{ scripts: {} }`).
    pub struct ThemeFontSet {
        req scripts: JsRecord<Str> = "scripts",
        opt latin: Str = "latin",
        opt ea: Str = "ea",
        opt cs: Str = "cs",
    }
}

js_object! {
    /// The Word theme (`Theme` of `readTheme`), built by R2 from
    /// [`uo_drawingml::theme::ThemePart`] with the Word rules.
    pub struct Theme {
        req major: ThemeFontSet = "major",
        req minor: ThemeFontSet = "minor",
        /// dk1, lt1, …, accent1..6, hlink, folHlink → RRGGBB
        req colors: JsRecord<Str> = "colors",
        opt line_widths_pt: Vec<JsNumber> = "lineWidthsPt",
        opt fill_styles: Vec<ShapeStyle> = "fillStyles",
        opt bg_fill_styles: Vec<ShapeStyle> = "bgFillStyles",
        opt effect_styles: Vec<Option<ShapeShadow>> = "effectStyles",
    }
}

impl uo_drawingml::dml::SchemeColors for JsRecord<Str> {
    fn scheme_color(&self, key: &str) -> Option<&str> {
        self.get(key).map(|v| &**v)
    }
}

js_object! {
    /// `w:footnotePr` / `w:endnotePr` (`NoteProps`).
    pub struct NoteProps {
        opt pos: Str = "pos",
        opt num_fmt: Str = "numFmt",
        opt num_start: JsNumber = "numStart",
        opt num_restart: Str = "numRestart",
    }
}

js_object! {
    /// `w:themeFontLang`.
    pub struct ThemeFontLang {
        opt val: Str = "val",
        opt east_asia: Str = "eastAsia",
        opt bidi: Str = "bidi",
    }
}

js_object! {
    /// `settings.xml` (`Settings`), in the assignment order of `readSettings`.
    pub struct Settings {
        req default_tab_stop: JsNumber = "defaultTabStop",
        req compatibility_mode: JsNumber = "compatibilityMode",
        req compat: JsRecord<bool> = "compat",
        req even_and_odd_headers: bool = "evenAndOddHeaders",
        req mirror_margins: bool = "mirrorMargins",
        req auto_hyphenation: bool = "autoHyphenation",
        req do_not_hyphenate_caps: bool = "doNotHyphenateCaps",
        req display_background_shape: bool = "displayBackgroundShape",
        req update_fields: bool = "updateFields",
        opt borders_do_not_surround_header: bool = "bordersDoNotSurroundHeader",
        opt borders_do_not_surround_footer: bool = "bordersDoNotSurroundFooter",
        opt footnote_pr: NoteProps = "footnotePr",
        opt endnote_pr: NoteProps = "endnotePr",
        opt hyphenation_zone: JsNumber = "hyphenationZone",
        opt consecutive_hyphen_limit: JsNumber = "consecutiveHyphenLimit",
        opt character_spacing_control: Str = "characterSpacingControl",
        opt theme_font_lang: ThemeFontLang = "themeFontLang",
    }
}

js_object! {
    /// `w:font` of the font table (`FontInfo`).
    pub struct FontInfo {
        req name: Str = "name",
        opt alt_name: Str = "altName",
        opt panose1: Str = "panose1",
        opt charset: Str = "charset",
        opt family: Str = "family",
        opt pitch: Str = "pitch",
        opt sig: JsRecord<Str> = "sig",
    }
}

js_object! {
    /// The formatting of the whole document (`DocFormatting`).
    pub struct DocFormatting {
        req styles: StyleSheet = "styles",
        req numbering: Numbering = "numbering",
        req theme: Theme = "theme",
        req settings: Settings = "settings",
        req font_table: JsRecord<FontInfo> = "fontTable",
    }
}

js_object! {
    /// A style resolved through `basedOn` and the defaults (`ResolvedStyle` of `styles.ts`).
    pub struct ResolvedStyle {
        req para: ParaProps = "para",
        req run: RunProps = "run",
        opt name: Str = "name",
        /// paragraph | character
        opt r#type: Str = "type",
    }
}

/// `StylesMap` (`Map<string, ResolvedStyle>`), in insertion order.
pub type StylesMap = JsMap<Str, ResolvedStyle>;
