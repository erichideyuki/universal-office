//! The shape of the generated preset table (`tables/presets.rs`, from `preset-defs.ts` as
//! `parseDef` of `preset-engine.ts` tokenizes it) and the lookup by name.

use crate::tables::presets::{PRESET_INDEX, PRESETS};

/// A guide formula operator (`evalFormula`); anything the TS `switch` does not know evaluates to 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Val,
    MulDiv,
    AddSub,
    AddDiv,
    IfElse,
    Abs,
    At2,
    Cat2,
    Sat2,
    Cos,
    Sin,
    Tan,
    Max,
    Min,
    Mod,
    Pin,
    Sqrt,
    Unknown,
}

impl Op {
    /// The operator named by a formula token (`val`, `*/`, `+-`, `+/`, `?:`, `abs`, `at2`,
    /// `cat2`, `sat2`, `cos`, `sin`, `tan`, `max`, `min`, `mod`, `pin`, `sqrt`); the match is
    /// exact like the TS `switch` (case sensitive, no trimming).
    pub(crate) fn parse(token: &str) -> Op {
        match token {
            "val" => Op::Val,
            "*/" => Op::MulDiv,
            "+-" => Op::AddSub,
            "+/" => Op::AddDiv,
            "?:" => Op::IfElse,
            "abs" => Op::Abs,
            "at2" => Op::At2,
            "cat2" => Op::Cat2,
            "sat2" => Op::Sat2,
            "cos" => Op::Cos,
            "sin" => Op::Sin,
            "tan" => Op::Tan,
            "max" => Op::Max,
            "min" => Op::Min,
            "mod" => Op::Mod,
            "pin" => Op::Pin,
            "sqrt" => Op::Sqrt,
            _ => Op::Unknown,
        }
    }
}

/// A named guide of a preset: the formula `[op, a, b, c]` with the missing arguments already the
/// token `"0"` (the TS default, resolved through `val`, so a custom guide named `"0"` is seen).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Guide {
    pub(crate) name: &'static str,
    pub(crate) op: Op,
    pub(crate) args: [&'static str; 3],
}

/// A path command: `op` is the byte of `M L Q C A Z`, or 0 for a command the TS ignores; `args`
/// are the tokens after it. A missing argument is `val(undefined)` = 0, NOT the token `"0"`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Cmd {
    pub(crate) op: u8,
    pub(crate) args: &'static [&'static str],
}

/// One `<a:path>`: its own coordinate system (`w`, `h`), the fill (`None` = `fill="none"`, else
/// the luminance offset of `darken`/`lighten`, 0 for no shading), the stroke flag and commands.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PathDef {
    pub(crate) w: Option<f64>,
    pub(crate) h: Option<f64>,
    pub(crate) fill: Option<f64>,
    pub(crate) stroke: bool,
    pub(crate) cmds: &'static [Cmd],
}

/// A parsed preset: adjust values, guides, paths and the text rectangle `[l, t, r, b]` tokens.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PresetDef {
    pub(crate) av: &'static [Guide],
    pub(crate) gd: &'static [Guide],
    pub(crate) paths: &'static [PathDef],
    pub(crate) rect: Option<[&'static str; 4]>,
}

pub(crate) const fn g(name: &'static str, op: Op, a: &'static str, b: &'static str, c: &'static str) -> Guide {
    Guide { name, op, args: [a, b, c] }
}

pub(crate) const fn c(op: u8, args: &'static [&'static str]) -> Cmd {
    Cmd { op, args }
}

pub(crate) const fn p(w: Option<f64>, h: Option<f64>, fill: Option<f64>, stroke: bool, cmds: &'static [Cmd]) -> PathDef {
    PathDef { w, h, fill, stroke, cmds }
}

/// The definition of `name`; own names of the table only, so `constructor` and the like are
/// unknown (G7).
pub(crate) fn lookup(name: &str) -> Option<&'static PresetDef> {
    let i = PRESET_INDEX.binary_search_by(|&i| PRESETS.get(usize::from(i)).map_or("", |e| e.0).cmp(name)).ok()?;
    PRESETS.get(usize::from(*PRESET_INDEX.get(i)?)).map(|e| &e.1)
}

/// Every preset name in the table (JS key) order.
pub(crate) fn names() -> impl Iterator<Item = &'static str> {
    PRESETS.iter().map(|e| e.0)
}
