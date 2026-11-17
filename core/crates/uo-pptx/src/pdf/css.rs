//! The CSS colours of the display list (the strings the TS assigned to `fillStyle`, `shadowColor`, gradient
//! stops...): `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()`, `transparent` and the colour names the
//! producers use. What cannot be read is `None` (the canvas ignores an assignment it cannot parse).

use uo_jsnum::conv::to_uint8_clamp;
use uo_jsnum::parse::parse_float;
use uo_jsnum::text::trim;

/// A colour as the canvas holds it: red, green and blue 0..255 and alpha 0..1.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rgba(pub f64, pub f64, pub f64, pub f64);

impl Rgba {
    pub const BLACK: Rgba = Rgba(0.0, 0.0, 0.0, 1.0);

    /// The 8-bit channels, rounded.
    #[must_use]
    pub fn bytes(self) -> [u8; 3] {
        [to_uint8_clamp(self.0), to_uint8_clamp(self.1), to_uint8_clamp(self.2)]
    }
}

fn hex(c: &str) -> Option<u32> {
    u32::from_str_radix(c, 16).ok()
}

/// Parses a CSS colour.
#[must_use]
pub fn parse_css(s: &str) -> Option<Rgba> {
    let t = trim(s);
    let lower: String = t.chars().flat_map(char::to_lowercase).collect();
    let t = lower.as_str();
    if let Some(h) = t.strip_prefix('#') {
        if !h.is_ascii() {
            return None;
        }
        let ch = |i: usize, w: usize| h.get(i..i + w).and_then(hex);
        return match h.len() {
            3 | 4 => {
                let d = |i| ch(i, 1).map(|v| f64::from(v * 17));
                let a = if h.len() == 4 { d(3)? / 255.0 } else { 1.0 };
                Some(Rgba(d(0)?, d(1)?, d(2)?, a))
            }
            6 | 8 => {
                let d = |i| ch(i, 2).map(f64::from);
                let a = if h.len() == 8 { d(6)? / 255.0 } else { 1.0 };
                Some(Rgba(d(0)?, d(2)?, d(4)?, a))
            }
            _ => None,
        };
    }
    if let Some(args) = t.strip_prefix("rgba(").or_else(|| t.strip_prefix("rgb(")) {
        let args = args.strip_suffix(')')?;
        let parts: Vec<&str> = args.split([',', '/', ' ']).filter(|p| !trim(p).is_empty()).collect();
        if parts.len() < 3 || parts.len() > 4 {
            return None;
        }
        let ch = |p: &str| -> Option<f64> {
            let p = trim(p);
            let v = if let Some(pc) = p.strip_suffix('%') { parse_float(pc) * 2.55 } else { parse_float(p) };
            (!v.is_nan()).then_some(v)
        };
        let a = match parts.get(3) {
            Some(p) => {
                let p = trim(p);
                let v = if let Some(pc) = p.strip_suffix('%') { parse_float(pc) / 100.0 } else { parse_float(p) };
                if v.is_nan() {
                    return None;
                }
                uo_jsnum::math::max(0.0, uo_jsnum::math::min(1.0, v))
            }
            None => 1.0,
        };
        return Some(Rgba(ch(parts.first()?)?, ch(parts.get(1)?)?, ch(parts.get(2)?)?, a));
    }
    let named = match t {
        "transparent" => return Some(Rgba(0.0, 0.0, 0.0, 0.0)),
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "green" => (0, 128, 0),
        "lime" => (0, 255, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "cyan" | "aqua" => (0, 255, 255),
        "magenta" | "fuchsia" => (255, 0, 255),
        "gray" | "grey" => (128, 128, 128),
        "silver" => (192, 192, 192),
        "orange" => (255, 165, 0),
        "purple" => (128, 0, 128),
        "maroon" => (128, 0, 0),
        "navy" => (0, 0, 128),
        "teal" => (0, 128, 128),
        "olive" => (128, 128, 0),
        _ => return None,
    };
    Some(Rgba(f64::from(named.0), f64::from(named.1), f64::from(named.2), 1.0))
}
