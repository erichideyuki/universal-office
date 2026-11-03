//! The page background (docx-core `page-background.ts`, read side: `readPageBackground`),
//! frontier R3.

use crate::js::{JsNumber, Str};
use crate::js_object;

use super::media::ImageData;

js_object! {
    /// A stop of `PageFill.colors`.
    pub struct FillStop {
        req pos: JsNumber = "pos",
        req color: Str = "color",
    }
}

js_object! {
    /// The fill effect of the page background (`PageFill`, `v:background/v:fill`).
    pub struct PageFill {
        /// gradient | gradientRadial | pattern | frame | tile
        req r#type: Str = "type",
        opt color: Str = "color",
        opt color2: Str = "color2",
        opt angle: JsNumber = "angle",
        opt focus: JsNumber = "focus",
        opt focus_position: (JsNumber, JsNumber) = "focusPosition",
        opt gradient_center: bool = "gradientCenter",
        opt colors: Vec<FillStop> = "colors",
        opt method: Str = "method",
        opt title: Str = "title",
        opt image: ImageData = "image",
        opt target_screen: bool = "targetScreen",
    }
}

js_object! {
    /// `w:background` (`PageBackground`).
    pub struct PageBackground {
        opt color: Str = "color",
        opt theme_color: Str = "themeColor",
        opt theme_tint: Str = "themeTint",
        opt theme_shade: Str = "themeShade",
        opt fill: Box<PageFill> = "fill",
    }
}
