//! Preset shapes: the generated definitions ([`def`]), the guide interpreter ([`engine`]) and the
//! path descriptor ([`path`]).

pub(crate) mod def;
mod engine;
mod path;

pub use engine::{
    GuideDef, Guides, PresetGeometry, TextRect, arc_to_seg, custom_guides, eval_preset, has_preset_def, preset_guides, preset_names,
    preset_text_rect,
};
pub use path::{PathShape, PresetPath, preset_path};
