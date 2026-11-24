//! `cube.ts`: the external-data functions without a source — CUBE* (no OLAP/data-model connection: `#N/A` after
//! the argument checks Excel makes) and STOCKHISTORY (validated, then `#BUSY!`: no quote service). The argument
//! modes (`any` at the member/set positions, `value` elsewhere, lifted) are the registry's.

use crate::fnreg::{CallArgs, FnBody, FnContext};
use crate::value::{ErrorCode, Scalar, Value};

use super::super::shared::{R, arg, bodies, scalar, to_num_d};

pub(super) const CUBE: &[(&str, FnBody)] = bodies![
    "CUBEMEMBER" => cube_plain,
    "CUBEVALUE" => cube_plain,
    "CUBEMEMBERPROPERTY" => cube_plain,
    "CUBERANKEDMEMBER" => cube_rank,
    "CUBEKPIMEMBER" => cube_rank,
    "CUBESET" => cube_plain,
    "CUBESETCOUNT" => cube_plain,
    "STOCKHISTORY" => stockhistory,
];

/// The CUBE* functions without a numeric argument.
fn cube_plain(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cube(a, ctx, &[])
}

/// CUBERANKEDMEMBER (the rank) and CUBEKPIMEMBER (the KPI property): position 2 must be a number.
fn cube_rank(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    cube(a, ctx, &[2])
}

/// `firstError(args)`: the first scalar error among the arguments (an array or a reference in an `any` position
/// does not count).
fn first_error(a: &CallArgs<'_>) -> Option<ErrorCode> {
    (0..a.args.len()).find_map(|i| match arg(a, i) {
        Some(Value::Scalar(Scalar::Err(e))) => Some(*e),
        _ => None,
    })
}

/// `cubeFn(…).call`: the first error, then the `numeric` positions must be numbers (text that is not → `#VALUE!`),
/// then `#N/A`.
fn cube(a: &CallArgs<'_>, ctx: &mut dyn FnContext, numeric: &[usize]) -> R<Value> {
    if let Some(e) = first_error(a) {
        return Err(e.into());
    }
    for &i in numeric {
        if let Some(Value::Scalar(s)) = arg(a, i) {
            to_num_d(ctx, *s)?;
        }
    }
    Err(ErrorCode::NA.into())
}

fn stockhistory(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    if let Some(e) = first_error(a) {
        return Err(e.into());
    }
    let ok = match scalar(a, 0) {
        Some(Scalar::Str(s)) => ctx.text(s).units().any(|u| !uo_jsnum::text::is_js_space_unit(u)),
        _ => false,
    };
    if !ok {
        return Err(ErrorCode::Value.into());
    }
    for i in [1, 2] {
        match scalar(a, i) {
            None | Some(Scalar::Empty) => {}
            Some(d) => {
                if to_num_d(ctx, d).is_err() {
                    return Err(ErrorCode::Value.into());
                }
            }
        }
    }
    Err(ErrorCode::Busy.into())
}
