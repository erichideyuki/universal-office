//! `functions/engineering.ts`: number bases (BIN2DEC…: 10 digits, two's complement on the 10th), bit functions
//! (0 ≤ n < 2^48), CONVERT (the unit table measured in Excel), DELTA/GESTEP, ERF/ERFC (Cody's CALERF) and the
//! Bessel functions (Numerical Recipes in C, 2nd ed., with Excel's measured coefficient quirks).
#![expect(
    clippy::excessive_precision,
    clippy::approx_constant,
    clippy::assign_op_pattern,
    reason = "the TS literals (Cody, Numerical Recipes) digit for digit, and its expressions as written"
)]

use uo_jsnum::math::{abs, exp, log, pow, sqrt, trunc};

use super::super::shared::mathu::{NumSpec, atp_fn, atp_num, atp_raw, lift_scalars, num_fn};
use super::super::shared::{EMPTY, R, Stop, arg, ascii, bodies, err, len, num};
use crate::fnreg::{CallArgs, FnBody, FnContext, Throw};
use crate::libm::{xl_cos, xl_sin};
use crate::math_util::{fin, fin_flush};
use crate::value::{ErrorCode, Scalar, Value};

/// The bodies of `engineering.ts`.
pub(super) const BODIES: &[(&str, FnBody)] = bodies![
    "BIN2OCT" => bin2oct,
    "BIN2DEC" => bin2dec,
    "BIN2HEX" => bin2hex,
    "OCT2BIN" => oct2bin,
    "OCT2DEC" => oct2dec,
    "OCT2HEX" => oct2hex,
    "DEC2BIN" => dec2bin,
    "DEC2OCT" => dec2oct,
    "DEC2HEX" => dec2hex,
    "HEX2BIN" => hex2bin,
    "HEX2OCT" => hex2oct,
    "HEX2DEC" => hex2dec,
    "BITAND" => bitand,
    "BITOR" => bitor,
    "BITXOR" => bitxor,
    "BITLSHIFT" => bitlshift,
    "BITRSHIFT" => bitrshift,
    "CONVERT" => convert,
    "DELTA" => delta,
    "GESTEP" => gestep,
    "ERF" => erf_fn,
    "ERF.PRECISE" => erf_precise,
    "ERFC" => erfc_fn,
    "ERFC.PRECISE" => erfc_precise,
    "BESSELJ" => besselj,
    "BESSELY" => bessely,
    "BESSELI" => besseli,
    "BESSELK" => besselk,
];

// ---------------------------------------------------------------- bases

/// `baseText(v)`: the text of a "number in base" argument (a non-negative integer number becomes its digits).
fn base_text(ctx: &dyn FnContext, v: Scalar) -> Result<Vec<u16>, ErrorCode> {
    match v {
        Scalar::Err(e) => Err(e),
        Scalar::Bool(_) => Err(ErrorCode::Value),
        Scalar::Empty => Ok(Vec::new()),
        Scalar::Num(x) => {
            if x < 0.0 || !uo_jsnum::conv::is_integer(x) {
                return Err(ErrorCode::Num);
            }
            if x >= 1e10 {
                return Err(ErrorCode::Num);
            }
            Ok(uo_jsnum::format::to_js_string(x).bytes().map(u16::from).collect())
        }
        Scalar::Str(r) => Ok(ctx.text(r).to_utf16()),
    }
}

const DIG: &[u8; 16] = b"0123456789ABCDEF";

/// `parseBase(text, radix)`: up to 10 digits, signed by two's complement on the 10th (the digits of the full
/// Unicode upper case: `HEX2DEC("ﬀ")` = 255).
fn parse_base(text: &[u16], radix: f64) -> Result<f64, ErrorCode> {
    if text.len() > 10 {
        return Err(ErrorCode::Num);
    }
    let up = crate::jstext::upper(text);
    let mut v = 0.0;
    for ch in char::decode_utf16(up.iter().copied()) {
        let d = match ch {
            Ok(c) if c.is_ascii() => DIG.iter().position(|&x| char::from(x) == c),
            _ => None,
        };
        let Some(d) = d else { return Err(ErrorCode::Num) };
        let d = d as f64;
        if d >= radix {
            return Err(ErrorCode::Num);
        }
        v = v * radix + d;
    }
    let full = pow(radix, 10.0);
    if text.len() == 10 && v >= full / 2.0 {
        v -= full;
    }
    Ok(v)
}

/// `formatBase(v, radix, places)`.
fn format_base(v: f64, radix: f64, places: Option<f64>) -> Result<Vec<u8>, ErrorCode> {
    let half = pow(radix, 10.0) / 2.0;
    let (lo, hi) = (-half, half - 1.0);
    if v < lo || v > hi {
        return Err(ErrorCode::Num);
    }
    let r = crate::cast::to_u32(radix);
    let digits = |x: f64| -> Vec<u8> {
        uo_jsnum::format::to_radix_string(x, r).map_or_else(|_| b"NaN".to_vec(), |s| s.into_bytes()).to_ascii_uppercase()
    };
    if v < 0.0 {
        return Ok(digits(v + pow(radix, 10.0)));
    }
    let s = digits(v);
    let Some(p) = places else { return Ok(s) };
    if p < s.len() as f64 || p > 10.0 {
        return Err(ErrorCode::Num);
    }
    // `padStart(NaN)` pads nothing
    let k = if p.is_nan() { 0 } else { crate::cast::trunc_usize(p) };
    let mut out = vec![b'0'; k.saturating_sub(s.len())];
    out.extend_from_slice(&s);
    Ok(out)
}

/// `baseConv(from, to)`: ATP argument (text or number), optional `places` when the target is not decimal.
fn base_conv(a: &CallArgs<'_>, ctx: &mut dyn FnContext, from: f64, to: f64) -> R<Value> {
    let has_places = to != 10.0;
    let mut raw = vec![atp_raw(Some(arg(a, 0).unwrap_or(&EMPTY)), ctx)?];
    if has_places && len(a) > 1 && !matches!(arg(a, 1), Some(Value::Scalar(Scalar::Empty)) | None) {
        raw.push(atp_raw(arg(a, 1), ctx)?);
    }
    lift_scalars(&raw, ctx, |s, ctx| {
        let av = s.first().copied().unwrap_or(Scalar::Empty);
        let v = if from == 10.0 {
            match atp_num(ctx, av) {
                Ok(x) => trunc(x),
                Err(e) => return Ok(err(e)),
            }
        } else {
            let t = match base_text(ctx, av) {
                Ok(t) => t,
                Err(e) => return Ok(err(e)),
            };
            match parse_base(&t, from) {
                Ok(v) => v,
                Err(e) => return Ok(err(e)),
            }
        };
        let mut places = None;
        if let Some(&p) = s.get(1) {
            let pn = match atp_num(ctx, p) {
                Ok(x) => x,
                Err(e) => return Ok(err(e)),
            };
            let q = trunc(pn);
            if q <= 0.0 {
                return Ok(err(ErrorCode::Num));
            }
            places = Some(q);
        }
        if to == 10.0 {
            return Ok(num(v));
        }
        match format_base(v, to, places) {
            Ok(t) => ascii(ctx, &t),
            Err(e) => Ok(err(e)),
        }
    })
}

fn bin2oct(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    base_conv(a, c, 2.0, 8.0)
}
fn bin2dec(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    base_conv(a, c, 2.0, 10.0)
}
fn bin2hex(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    base_conv(a, c, 2.0, 16.0)
}
fn oct2bin(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    base_conv(a, c, 8.0, 2.0)
}
fn oct2dec(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    base_conv(a, c, 8.0, 10.0)
}
fn oct2hex(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    base_conv(a, c, 8.0, 16.0)
}
fn dec2bin(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    base_conv(a, c, 10.0, 2.0)
}
fn dec2oct(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    base_conv(a, c, 10.0, 8.0)
}
fn dec2hex(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    base_conv(a, c, 10.0, 16.0)
}
fn hex2bin(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    base_conv(a, c, 16.0, 2.0)
}
fn hex2oct(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    base_conv(a, c, 16.0, 8.0)
}
fn hex2dec(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    base_conv(a, c, 16.0, 10.0)
}

// ---------------------------------------------------------------- bits

const BIT_LIMIT: f64 = 281_474_976_710_656.0; // 2^48

/// `bitArg(n)`: an integer 0 ≤ n < 2^48 (else `#NUM!`).
fn bit_arg(x: f64) -> Result<u128, ErrorCode> {
    if x < 0.0 || x >= BIT_LIMIT || !uo_jsnum::conv::is_integer(x) {
        return Err(ErrorCode::Num);
    }
    Ok(u128::from(crate::cast::trunc_u64(x)))
}

fn bit_op(a: &CallArgs<'_>, ctx: &mut dyn FnContext, op: fn(u128, u128) -> u128) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(2, 2), |xs, _| {
        let x = bit_arg(xs.at(0));
        let y = bit_arg(xs.at(1));
        let x = x?;
        let y = y?;
        Ok(num(u128_to_f64(op(x, y))))
    })
}

/// A value below 2^53 as a double (exact).
fn u128_to_f64(x: u128) -> f64 {
    crate::cast::u53_to_f64(u64::try_from(x).unwrap_or(0) & ((1u64 << 53) - 1))
}

fn bitand(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    bit_op(a, c, |x, y| x & y)
}
fn bitor(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    bit_op(a, c, |x, y| x | y)
}
fn bitxor(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    bit_op(a, c, |x, y| x ^ y)
}

/// `shift(n, s)` (measured): n = 0 gives 0 for any shift; |s| > 53 = `#NUM!`; to the left the result is taken
/// modulo 2^53 before the 2^48 test. A NaN shift reaches `BigInt(NaN)`: a `RangeError` (thrown, as the TS).
fn shift(nv: f64, s: f64) -> R<Value> {
    let x = bit_arg(nv)?;
    if x == 0 {
        return Ok(num(0.0));
    }
    let k = trunc(s);
    if abs(k) > 53.0 {
        return Ok(err(ErrorCode::Num));
    }
    if k.is_nan() {
        return Err(Stop::Throw(Throw::Range));
    }
    let r = if k >= 0.0 { (x << crate::cast::trunc_u64(k)) % (1u128 << 53) } else { x >> crate::cast::trunc_u64(-k) };
    if r >= (1u128 << 48) {
        return Ok(err(ErrorCode::Num));
    }
    Ok(num(u128_to_f64(r)))
}

fn bitlshift(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(2, 2), |xs, _| shift(xs.at(0), xs.at(1)))
}

fn bitrshift(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    num_fn(a, ctx, &NumSpec::new(2, 2), |xs, _| shift(xs.at(0), -xs.at(1)))
}

// ---------------------------------------------------------------- CONVERT

/// Unit categories; `Proto` is what a name of `Object.prototype` (`constructor`, `toString`, `__proto__`…) reads
/// from the TS unit tables: a truthy entry whose category and factor are `undefined`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cat {
    Mass,
    Dist,
    Time,
    Press,
    Force,
    Energy,
    Power,
    Magn,
    Temp,
    Vol,
    Area,
    Info,
    Speed,
    Proto,
}

use Cat::*;

/// Units per one base unit of the category (g, m, sec, Pa, N, J, W, T, l, m2, bit, m/s), read from Excel with
/// `CONVERT(1;base;u)` (exact bits).
const UNITS: &[(&str, Cat, f64)] = &[
    ("g", Mass, 1.0),
    ("sg", Mass, 0.000_068_521_765_856_791_77),
    ("lbm", Mass, 0.002_204_622_621_848_776),
    ("u", Mass, 6.022_141_794_216_764e23),
    ("ozm", Mass, 0.035_273_961_949_580_414),
    ("grain", Mass, 15.432_358_352_941_43),
    ("cwt", Mass, 0.000_022_046_226_218_487_758),
    ("shweight", Mass, 0.000_022_046_226_218_487_758),
    ("uk_cwt", Mass, 0.000_019_684_130_552_221_215),
    ("lcwt", Mass, 0.000_019_684_130_552_221_215),
    ("hweight", Mass, 0.000_019_684_130_552_221_215),
    ("stone", Mass, 0.000_157_473_044_417_769_72),
    ("ton", Mass, 0.000_001_102_311_310_924_388),
    ("uk_ton", Mass, 9.842_065_276_110_607e-7),
    ("LTON", Mass, 9.842_065_276_110_607e-7),
    ("brton", Mass, 9.842_065_276_110_607e-7),
    ("m", Dist, 1.0),
    ("mi", Dist, 0.000_621_371_192_237_333_9),
    ("Nmi", Dist, 0.000_539_956_803_455_723_6),
    ("in", Dist, 39.370_078_740_157_48),
    ("ft", Dist, 3.280_839_895_013_123_5),
    ("yd", Dist, 1.093_613_298_337_707_8),
    ("ang", Dist, 10_000_000_000.0),
    ("ell", Dist, 0.874_890_638_670_166_3),
    ("ly", Dist, 1.057_000_834_024_615_4e-16),
    ("parsec", Dist, 3.240_779_289_664_725e-17),
    ("pc", Dist, 3.240_779_289_664_725e-17),
    ("Picapt", Dist, 2_834.645_669_291_338_4),
    ("Pica", Dist, 2_834.645_669_291_338_4),
    ("pica", Dist, 236.220_472_440_944_86),
    ("survey_mi", Dist, 0.000_621_369_949_494_949_5),
    ("sec", Time, 1.0),
    ("yr", Time, 3.168_808_781_402_895e-8),
    ("day", Time, 0.000_011_574_074_074_074_073),
    ("d", Time, 0.000_011_574_074_074_074_073),
    ("hr", Time, 0.000_277_777_777_777_777_8),
    ("mn", Time, 0.016_666_666_666_666_666),
    ("min", Time, 0.016_666_666_666_666_666),
    ("s", Time, 1.0),
    ("Pa", Press, 1.0),
    ("p", Press, 1.0),
    ("atm", Press, 0.000_009_869_232_667_160_129),
    ("at", Press, 0.000_009_869_232_667_160_129),
    ("mmHg", Press, 0.007_500_637_554_192_106),
    ("psi", Press, 0.000_145_037_737_730_209_2),
    ("Torr", Press, 0.007_500_616_827_041_697),
    ("N", Force, 1.0),
    ("dyn", Force, 100_000.0),
    ("dy", Force, 100_000.0),
    ("lbf", Force, 0.224_808_943_099_710_47),
    ("pond", Force, 101.971_621_297_792_82),
    ("J", Energy, 1.0),
    ("e", Energy, 10_000_000.0),
    ("c", Energy, 0.239_005_736_137_667_3),
    ("cal", Energy, 0.238_845_896_627_495_95),
    ("eV", Energy, 6_241_509_647_120_418_000.0),
    ("ev", Energy, 6_241_509_647_120_418_000.0),
    ("HPh", Energy, 3.725_061_359_986_188e-7),
    ("hh", Energy, 3.725_061_359_986_188e-7),
    ("Wh", Energy, 0.000_277_777_777_777_777_8),
    ("wh", Energy, 0.000_277_777_777_777_777_8),
    ("flb", Energy, 0.737_562_149_277_265_4),
    ("BTU", Energy, 0.000_947_817_120_313_317_1),
    ("btu", Energy, 0.000_947_817_120_313_317_1),
    ("W", Power, 1.0),
    ("HP", Power, 0.001_341_022_089_595_027_9),
    ("h", Power, 0.001_341_022_089_595_027_9),
    ("PS", Power, 0.001_359_621_617_303_904_3),
    ("w", Power, 1.0),
    ("T", Magn, 1.0),
    ("ga", Magn, 10_000.0),
    ("l", Vol, 1.0),
    ("m3", Vol, 0.001),
    ("tsp", Vol, 202.884_136_211_057_98),
    ("tspm", Vol, 200.0),
    ("tbs", Vol, 67.628_045_403_686),
    ("oz", Vol, 33.814_022_701_843),
    ("cup", Vol, 4.226_752_837_730_375),
    ("pt", Vol, 2.113_376_418_865_187_6),
    ("us_pt", Vol, 2.113_376_418_865_187_6),
    ("uk_pt", Vol, 1.759_753_986_392_702),
    ("qt", Vol, 1.056_688_209_432_593_8),
    ("uk_qt", Vol, 0.879_876_993_196_351),
    ("gal", Vol, 0.264_172_052_358_148_45),
    ("uk_gal", Vol, 0.219_969_248_299_087_76),
    ("L", Vol, 1.0),
    ("lt", Vol, 1.0),
    ("ang3", Vol, 1e27),
    ("ang^3", Vol, 1e27),
    ("barrel", Vol, 0.006_289_810_770_432_105),
    ("bushel", Vol, 0.028_377_593_258_401_747),
    ("ft3", Vol, 0.035_314_666_721_488_586),
    ("ft^3", Vol, 0.035_314_666_721_488_586),
    ("in3", Vol, 61.023_744_094_732_29),
    ("in^3", Vol, 61.023_744_094_732_29),
    ("ly3", Vol, 1.180_934_988_441_708_4e-51),
    ("ly^3", Vol, 1.180_934_988_441_708_4e-51),
    ("m^3", Vol, 0.001),
    ("mi3", Vol, 2.399_127_585_789_277_3e-13),
    ("mi^3", Vol, 2.399_127_585_789_277_3e-13),
    ("yd3", Vol, 0.001_307_950_619_314_392),
    ("yd^3", Vol, 0.001_307_950_619_314_392),
    ("Nmi3", Vol, 1.574_262_146_858_115e-13),
    ("Nmi^3", Vol, 1.574_262_146_858_115e-13),
    ("Picapt3", Vol, 22_776_990.435_870_633),
    ("Picapt^3", Vol, 22_776_990.435_870_633),
    ("Pica3", Vol, 22_776_990.435_870_633),
    ("Pica^3", Vol, 22_776_990.435_870_633),
    ("GRT", Vol, 0.000_353_146_667_214_885_86),
    ("regton", Vol, 0.000_353_146_667_214_885_86),
    ("MTON", Vol, 0.000_882_866_668_037_214_7),
    ("m2", Area, 1.0),
    ("uk_acre", Area, 0.000_247_105_381_467_165_3),
    ("us_acre", Area, 0.000_247_104_393_046_627_9),
    ("ang2", Area, 100_000_000_000_000_000_000.0),
    ("ang^2", Area, 100_000_000_000_000_000_000.0),
    ("ar", Area, 0.01),
    ("ft2", Area, 10.763_910_416_709_722),
    ("ft^2", Area, 10.763_910_416_709_722),
    ("ha", Area, 0.0001),
    ("in2", Area, 1_550.003_100_006_2),
    ("in^2", Area, 1_550.003_100_006_2),
    ("ly2", Area, 1.117_250_763_128_733e-32),
    ("ly^2", Area, 1.117_250_763_128_733e-32),
    ("m^2", Area, 1.0),
    ("Morgen", Area, 0.0004),
    ("mi2", Area, 3.861_021_585_424_458e-7),
    ("mi^2", Area, 3.861_021_585_424_458e-7),
    ("Nmi2", Area, 2.915_533_495_981_228_5e-7),
    ("Nmi^2", Area, 2.915_533_495_981_228_5e-7),
    ("Picapt2", Area, 8_035_216.070_432_141),
    ("Pica2", Area, 8_035_216.070_432_141),
    ("Pica^2", Area, 8_035_216.070_432_141),
    ("Picapt^2", Area, 8_035_216.070_432_141),
    ("yd2", Area, 1.195_990_046_301_080_2),
    ("yd^2", Area, 1.195_990_046_301_080_2),
    ("bit", Info, 1.0),
    ("byte", Info, 0.125),
    ("m/s", Speed, 1.0),
    ("admkn", Speed, 1.942_602_569_415_665_2),
    ("kn", Speed, 1.943_844_492_440_604_6),
    ("m/h", Speed, 3600.0),
    ("m/hr", Speed, 3600.0),
    ("m/sec", Speed, 1.0),
    ("mph", Speed, 2.236_936_292_054_402_5),
    ("K", Temp, 1.0),
    ("kel", Temp, 1.0),
    ("C", Temp, f64::NAN),
    ("cel", Temp, f64::NAN),
    ("F", Temp, f64::NAN),
    ("fah", Temp, f64::NAN),
    ("Rank", Temp, f64::NAN),
    ("Reau", Temp, f64::NAN),
];

/// The own properties of `Object.prototype` in V8: a lookup by one of these names finds a truthy inherited value.
const PROTO: [&str; 12] = [
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "__proto__",
    "toLocaleString",
];

/// Units that take a decimal prefix (measured) and the power of the prefix (area/volume).
const PREFIXABLE: &[(&str, f64)] = &[
    ("g", 1.0),
    ("u", 1.0),
    ("m", 1.0),
    ("ang", 1.0),
    ("ly", 1.0),
    ("parsec", 1.0),
    ("pc", 1.0),
    ("sec", 1.0),
    ("s", 1.0),
    ("Pa", 1.0),
    ("p", 1.0),
    ("atm", 1.0),
    ("at", 1.0),
    ("mmHg", 1.0),
    ("N", 1.0),
    ("dyn", 1.0),
    ("dy", 1.0),
    ("pond", 1.0),
    ("J", 1.0),
    ("e", 1.0),
    ("c", 1.0),
    ("cal", 1.0),
    ("eV", 1.0),
    ("ev", 1.0),
    ("Wh", 1.0),
    ("wh", 1.0),
    ("W", 1.0),
    ("w", 1.0),
    ("T", 1.0),
    ("ga", 1.0),
    ("K", 1.0),
    ("kel", 1.0),
    ("uk_pt", 1.0),
    ("l", 1.0),
    ("L", 1.0),
    ("lt", 1.0),
    ("ar", 1.0),
    ("bit", 1.0),
    ("byte", 1.0),
    ("m/s", 1.0),
    ("m/h", 1.0),
    ("m/hr", 1.0),
    ("m/sec", 1.0),
    ("mph", 1.0),
    ("m3", 3.0),
    ("m^3", 3.0),
    ("ang3", 3.0),
    ("ang^3", 3.0),
    ("m2", 2.0),
    ("m^2", 2.0),
    ("ang2", 2.0),
    ("ang^2", 2.0),
];

const PREFIXES: [(&str, f64); 21] = [
    ("da", 1e1),
    ("Y", 1e24),
    ("Z", 1e21),
    ("E", 1e18),
    ("P", 1e15),
    ("T", 1e12),
    ("G", 1e9),
    ("M", 1e6),
    ("k", 1e3),
    ("h", 1e2),
    ("e", 1e1),
    ("d", 1e-1),
    ("c", 1e-2),
    ("m", 1e-3),
    ("u", 1e-6),
    ("n", 1e-9),
    ("p", 1e-12),
    ("f", 1e-15),
    ("a", 1e-18),
    ("z", 1e-21),
    ("y", 1e-24),
];

const BIN_PREFIXES: [(&str, f64); 8] = [
    ("Yi", 1_208_925_819_614_629_174_706_176.0),
    ("Zi", 1_180_591_620_717_411_303_424.0),
    ("Ei", 1_152_921_504_606_846_976.0),
    ("Pi", 1_125_899_906_842_624.0),
    ("Ti", 1_099_511_627_776.0),
    ("Gi", 1_073_741_824.0),
    ("Mi", 1_048_576.0),
    ("ki", 1024.0),
];

/// `s === name` for units and an ASCII name.
fn eq(s: &[u16], name: &str) -> bool {
    s.len() == name.len() && s.iter().zip(name.bytes()).all(|(&a, b)| a == u16::from(b))
}

/// `UNITS[s]`: the entry (category, factor), the inherited `Object.prototype` value included.
fn unit_entry(s: &[u16]) -> Option<(Cat, f64)> {
    if let Some(&(_, c, u)) = UNITS.iter().find(|(n, _, _)| eq(s, n)) {
        return Some((c, u));
    }
    PROTO.iter().any(|n| eq(s, n)).then_some((Proto, f64::NAN))
}

/// `PREFIXABLE[r]` (an inherited value is truthy and its power NaN).
fn prefix_power(s: &[u16]) -> Option<f64> {
    if let Some(&(_, p)) = PREFIXABLE.iter().find(|(n, _)| eq(s, n)) {
        return Some(p);
    }
    PROTO.iter().any(|n| eq(s, n)).then_some(f64::NAN)
}

/// A unit (with prefix): the name without prefix, the category, the factor and the prefix factor.
#[derive(Clone, Copy, Debug)]
struct Unit<'a> {
    name: &'a [u16],
    cat: Cat,
    u: f64,
    pf: f64,
}

/// `parseUnit(s)`: the exact name first, then binary prefixes (bit/byte), then decimal prefixes in table order.
fn parse_unit(s: &[u16]) -> Option<Unit<'_>> {
    if let Some((cat, u)) = unit_entry(s) {
        return Some(Unit { name: s, cat, u, pf: 1.0 });
    }
    let starts = |p: &str| s.len() >= p.len() && eq(&s[..p.len()], p);
    for (p, f) in BIN_PREFIXES {
        if starts(p) {
            let r = &s[p.len()..];
            if eq(r, "bit") || eq(r, "byte") {
                let (_, u) = unit_entry(r)?;
                return Some(Unit { name: r, cat: Info, u, pf: f });
            }
        }
    }
    for (p, f) in PREFIXES {
        if !starts(p) {
            continue;
        }
        let r = &s[p.len()..];
        if let (Some((cat, u)), Some(pw)) = (unit_entry(r), prefix_power(r)) {
            return Some(Unit { name: r, cat, u, pf: pow(f, pw) });
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TempUnit {
    C,
    F,
    K,
    Rank,
    Reau,
}

fn temp_alias(s: &[u16]) -> TempUnit {
    if eq(s, "C") || eq(s, "cel") {
        TempUnit::C
    } else if eq(s, "F") || eq(s, "fah") {
        TempUnit::F
    } else if eq(s, "K") || eq(s, "kel") {
        TempUnit::K
    } else if eq(s, "Rank") {
        TempUnit::Rank
    } else {
        TempUnit::Reau
    }
}

/// The direct formula of each pair (measured bit by bit).
fn convert_temp(x: f64, a: TempUnit, b: TempUnit) -> f64 {
    use TempUnit as T;
    if a == b {
        return x;
    }
    match a {
        T::C => match b {
            T::F => x * 1.8 + 32.0,
            T::K => x + 273.15,
            T::Rank => (x + 273.15) * 1.8,
            _ => x * 0.8,
        },
        T::F => match b {
            T::C => (x - 32.0) / 1.8,
            T::K => (x - 32.0) / 1.8 + 273.15,
            T::Rank => x + 459.67,
            _ => (x - 32.0) * 5.0 / 9.0 * 0.8,
        },
        T::K => match b {
            T::C => x - 273.15,
            T::F => (x - 273.15) * 1.8 + 32.0,
            T::Rank => x * 1.8,
            _ => (x - 273.15) * 0.8,
        },
        T::Rank => match b {
            T::C => x / 1.8 - 273.15,
            T::F => x - 459.67,
            T::K => x / 1.8,
            _ => (x / 1.8 - 273.15) * 0.8,
        },
        T::Reau => {
            let c = x / 0.8;
            match b {
                T::C => c,
                T::F => c * 1.8 + 32.0,
                T::K => c + 273.15,
                _ => (c + 273.15) * 1.8,
            }
        }
    }
}

/// `convertUnits(x, from, to)`.
fn convert_units(x: f64, from: &[u16], to: &[u16]) -> Result<f64, ErrorCode> {
    let (Some(a), Some(b)) = (parse_unit(from), parse_unit(to)) else { return Err(ErrorCode::NA) };
    if a.cat != b.cat {
        return Err(ErrorCode::NA);
    }
    if a.cat == Temp {
        let (ta, tb) = (temp_alias(a.name), temp_alias(b.name));
        // measured: the kelvin prefix is ignored from mK/kK to Rank and between Reau and mK/kK
        let ignore = (ta == TempUnit::K && (tb == TempUnit::Rank || tb == TempUnit::Reau)) || (tb == TempUnit::K && ta == TempUnit::Reau);
        let xa = if ignore { x } else { x * a.pf };
        let r = convert_temp(xa, ta, tb);
        return fin(if ignore { r } else { r / b.pf });
    }
    if from == to {
        return Ok(x);
    }
    fin(x * (b.u / a.u) * a.pf / b.pf)
}

fn convert(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    let mut raw = Vec::with_capacity(3);
    for i in 0..len(a) {
        raw.push(atp_raw(Some(arg(a, i).unwrap_or(&EMPTY)), ctx)?);
    }
    lift_scalars(&raw, ctx, |s, ctx| {
        let g = |i: usize| s.get(i).copied().unwrap_or(Scalar::Empty);
        let x = match atp_num(ctx, g(0)) {
            Ok(x) => x,
            Err(e) => return Ok(err(e)),
        };
        for u in [g(1), g(2)] {
            match u {
                Scalar::Err(e) => return Ok(err(e)),
                Scalar::Bool(_) => return Ok(err(ErrorCode::Value)),
                _ => {}
            }
        }
        let (Scalar::Str(f), Scalar::Str(t)) = (g(1), g(2)) else { return Ok(err(ErrorCode::NA)) };
        let (fu, tu) = (ctx.text(f).to_utf16(), ctx.text(t).to_utf16());
        Ok(match convert_units(x, &fu, &tu) {
            Ok(v) => num(v),
            Err(e) => err(e),
        })
    })
}

// ---------------------------------------------------------------- DELTA / GESTEP

fn delta(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(1, 2).defaults(&[0.0]), |xs, _| Ok(num(if xs.at(0) == xs.at(1) { 1.0 } else { 0.0 })))
}

fn gestep(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(1, 2).defaults(&[0.0]), |xs, _| Ok(num(if xs.at(0) >= xs.at(1) { 1.0 } else { 0.0 })))
}

// ---------------------------------------------------------------- ERF / ERFC (W. J. Cody, SPECFUN CALERF)

const EA: [f64; 5] = [
    3.161_123_743_870_565_6e0,
    1.138_641_541_510_501_56e2,
    3.774_852_376_853_020_21e2,
    3.209_377_589_138_469_47e3,
    1.857_777_061_846_031_53e-1,
];
const EB: [f64; 4] = [2.360_129_095_234_412_09e1, 2.440_246_379_344_441_73e2, 1.282_616_526_077_372_28e3, 2.844_236_833_439_170_62e3];
const EC: [f64; 9] = [
    5.641_884_969_886_700_89e-1,
    8.883_149_794_388_375_94e0,
    6.611_919_063_714_162_95e1,
    2.986_351_381_974_001_31e2,
    8.819_522_212_417_690_90e2,
    1.712_047_612_634_070_58e3,
    2.051_078_377_826_071_47e3,
    1.230_339_354_797_997_25e3,
    2.153_115_354_744_038_46e-8,
];
const ED: [f64; 8] = [
    1.574_492_611_070_983_47e1,
    1.176_939_508_913_124_99e2,
    5.371_811_018_620_098_58e2,
    1.621_389_574_566_690_19e3,
    3.290_799_235_733_459_63e3,
    4.362_619_090_143_247_16e3,
    3.439_367_674_143_721_64e3,
    1.230_339_354_803_749_42e3,
];
const EP: [f64; 6] = [
    3.053_266_349_612_323_44e-1,
    3.603_448_999_498_044_39e-1,
    1.257_817_261_112_292_46e-1,
    1.608_378_514_874_227_66e-2,
    6.587_491_615_298_378_03e-4,
    1.631_538_713_730_209_78e-2,
];
const EQ: [f64; 5] = [
    2.568_520_192_289_822_42e0,
    1.872_952_849_923_467_25e0,
    5.279_051_029_514_284_12e-1,
    6.051_834_131_244_131_91e-2,
    2.335_204_976_268_691_85e-3,
];
const SQRPI: f64 = 5.641_895_835_477_562_869_5e-1;

/// `calerf(x, jint)`: erf (jint = 0) or erfc (jint = 1).
fn calerf(x: f64, erfc: bool) -> f64 {
    let y = abs(x);
    let mut result;
    if y <= 0.46875 {
        let ysq = if y > 1.11e-16 { y * y } else { 0.0 };
        let mut xnum = EA[4] * ysq;
        let mut xden = ysq;
        for i in 0..3 {
            xnum = (xnum + EA[i]) * ysq;
            xden = (xden + EB[i]) * ysq;
        }
        result = x * (xnum + EA[3]) / (xden + EB[3]);
        return if erfc { 1.0 - result } else { result };
    }
    if y <= 4.0 {
        let mut xnum = EC[8] * y;
        let mut xden = y;
        for i in 0..7 {
            xnum = (xnum + EC[i]) * y;
            xden = (xden + ED[i]) * y;
        }
        result = (xnum + EC[7]) / (xden + ED[7]);
        let ysq = trunc(y * 16.0) / 16.0;
        let del = (y - ysq) * (y + ysq);
        result = exp(-ysq * ysq) * exp(-del) * result;
    } else {
        result = 0.0;
        if y < 26.543 {
            let ysq0 = 1.0 / (y * y);
            let mut xnum = EP[5] * ysq0;
            let mut xden = ysq0;
            for i in 0..4 {
                xnum = (xnum + EP[i]) * ysq0;
                xden = (xden + EQ[i]) * ysq0;
            }
            result = ysq0 * (xnum + EP[4]) / (xden + EQ[4]);
            result = (SQRPI - result) / y;
            let ysq = trunc(y * 16.0) / 16.0;
            let del = (y - ysq) * (y + ysq);
            result = exp(-ysq * ysq) * exp(-del) * result;
        }
    }
    // result = erfc(|x|)
    if !erfc {
        let r = 0.5 - result + 0.5;
        return if x < 0.0 { -r } else { r };
    }
    if x < 0.0 { 2.0 - result } else { result }
}

/// `erf(x)` of `engineering.ts`.
#[must_use]
pub fn erf(x: f64) -> f64 {
    calerf(x, false)
}

/// `erfc(x)` of `engineering.ts`.
#[must_use]
pub fn erfc(x: f64) -> f64 {
    calerf(x, true)
}

fn erf_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    // measured: ERF(a;b) = ERFC(a) − ERFC(b) (bit by bit)
    atp_fn(a, ctx, &NumSpec::new(1, 2), |xs, _| Ok(num(if xs.len() > 1 { erfc(xs.at(0)) - erfc(xs.at(1)) } else { erf(xs.at(0)) })))
}

fn erf_precise(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| Ok(num(erf(xs.at(0)))))
}

fn erfc_fn(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| Ok(num(erfc(xs.at(0)))))
}

fn erfc_precise(a: &CallArgs<'_>, ctx: &mut dyn FnContext) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(1, 1), |xs, _| Ok(num(erfc(xs.at(0)))))
}

// ---------------------------------------------------------------- Bessel (Numerical Recipes in C, 2nd ed.)

/// The step budget of the Bessel recurrences (decision A4a-3): 2^22 steps (≈ 10 ms), an order up to ≈ 2·10^6.
pub const BESSEL_STEPS: u32 = 1 << 22;

/// A loop budget: `None` past [`BESSEL_STEPS`] steps, where the TS runs on (`BESSELJ(x; ∞)` never returns; a
/// finite huge order takes the TS seconds per cell).
struct Budget(u32);

impl Budget {
    fn step(&mut self) -> Option<()> {
        self.0 += 1;
        (self.0 <= BESSEL_STEPS).then_some(())
    }
}

/// A loop of `steps` iterations would pass the budget: answered at once with the budget's result (`#NUM!`; for the
/// upward recurrences an earlier non-finite value is `#NUM!` too), so a formula of many hostile calls stays cheap
/// (found by the fuzz run: 400 calls at the budget took 4 s).
fn over_budget(steps: f64) -> bool {
    steps > f64::from(BESSEL_STEPS)
}

/// `n & 1` of a JS number (`ToInt32`).
fn odd(n: f64) -> bool {
    uo_jsnum::conv::to_int32(n) & 1 != 0
}

fn bessj0(x: f64) -> f64 {
    let ax = abs(x);
    if ax < 8.0 {
        let y = x * x;
        let a1 = 57_568_490_574.0
            + y * (-13_362_590_354.0 + y * (651_619_640.7 + y * (-11_214_424.18 + y * (77_392.330_17 + y * (-184.905_245_6)))));
        let a2 = 57_568_490_411.0 + y * (1_029_532_985.0 + y * (9_494_680.718 + y * (59_272.648_53 + y * (267.853_271_2 + y * 1.0))));
        return a1 / a2;
    }
    // measured (exact fit to 80 Excel values, error 8E-17): Excel has −0,1098628267E-2 where Numerical Recipes has
    // −0,1098628627E-2 (swapped digits) — only in J0
    let z = 8.0 / ax;
    let y = z * z;
    let xx = ax - 0.785_398_164;
    let a1 = 1.0 + y * (-0.109_862_826_7e-2 + y * (0.273_451_040_7e-4 + y * (-0.207_337_063_9e-5 + y * 0.209_388_721_1e-6)));
    let a2 = -0.156_249_999_5e-1 + y * (0.143_048_876_5e-3 + y * (-0.691_114_765_1e-5 + y * (0.762_109_516_1e-6 - y * 0.934_935_152e-7)));
    sqrt(0.636_619_772 / ax) * (xl_cos(xx) * a1 - z * xl_sin(xx) * a2)
}

fn bessj1(x: f64) -> f64 {
    let ax = abs(x);
    if ax < 8.0 {
        let y = x * x;
        let a1 = x
            * (72_362_614_232.0
                + y * (-7_895_059_235.0 + y * (242_396_853.1 + y * (-2_972_611.439 + y * (15_704.482_60 + y * (-30.160_366_06))))));
        let a2 = 144_725_228_442.0 + y * (2_300_535_178.0 + y * (18_583_304.74 + y * (99_447.433_94 + y * (376.999_139_7 + y * 1.0))));
        return a1 / a2;
    }
    // measured (exact fit to 80 Excel values, error 3E-16): in J1 Excel repeats the term −0,3516396496E-4 (in y² and
    // y³, pushing the next ones one degree)
    let z = 8.0 / ax;
    let y = z * z;
    let xx = ax - 2.356_194_491;
    let a1 = 1.0
        + y * (0.183_105e-2 + y * (-0.351_639_649_6e-4 + y * (-0.351_639_649_6e-4 + y * (0.245_752_017_4e-5 + y * (-0.240_337_019e-6)))));
    let a2 = 0.046_874_999_95 + y * (-0.200_269_087_3e-3 + y * (0.844_919_909_6e-5 + y * (-0.882_289_87e-6 + y * 0.105_787_412e-6)));
    let r = sqrt(0.636_619_772 / ax) * (xl_cos(xx) * a1 - z * xl_sin(xx) * a2);
    if x < 0.0 { -r } else { r }
}

fn bessj(n: f64, x: f64) -> Option<f64> {
    if n == 0.0 {
        return Some(bessj0(x));
    }
    if n == 1.0 {
        return Some(bessj1(x));
    }
    let ax = abs(x);
    if ax == 0.0 {
        return Some(0.0);
    }
    let mut b = Budget(0);
    let tox = 2.0 / ax;
    let mut ans;
    if ax > n {
        if over_budget(n - 1.0) {
            return None;
        }
        let mut bjm = bessj0(ax);
        let mut bj = bessj1(ax);
        let mut j = 1.0;
        while j < n {
            b.step()?;
            let bjp = j * tox * bj - bjm;
            bjm = bj;
            bj = bjp;
            j += 1.0;
        }
        ans = bj;
    } else {
        let m = 2.0 * trunc((n + trunc(sqrt(40.0 * n))) / 2.0);
        if over_budget(m) {
            return None;
        }
        let mut jsum = false;
        let (mut bjp, mut sum, mut bj) = (0.0, 0.0, 1.0);
        ans = 0.0;
        let mut j = m;
        while j > 0.0 {
            b.step()?;
            let bjm = j * tox * bj - bjp;
            bjp = bj;
            bj = bjm;
            if abs(bj) > 1e10 {
                bj *= 1e-10;
                bjp *= 1e-10;
                ans *= 1e-10;
                sum *= 1e-10;
            }
            if jsum {
                sum += bj;
            }
            jsum = !jsum;
            if j == n {
                ans = bjp;
            }
            j -= 1.0;
        }
        sum = 2.0 * sum - bj;
        ans /= sum;
    }
    Some(if x < 0.0 && odd(n) { -ans } else { ans })
}

fn bessy0(x: f64) -> f64 {
    if x < 8.0 {
        let y = x * x;
        let a1 = -2_957_821_389.0
            + y * (7_062_834_065.0 + y * (-512_359_803.6 + y * (10_879_881.29 + y * (-86_327.927_57 + y * 228.462_273_3))));
        let a2 = 40_076_544_269.0 + y * (745_249_964.8 + y * (7_189_466.438 + y * (47_447.264_70 + y * (226.103_024_4 + y * 1.0))));
        return a1 / a2 + 0.636_619_772 * bessj0(x) * log(x);
    }
    let z = 8.0 / x;
    let y = z * z;
    let xx = x - 0.785_398_164;
    let a1 = 1.0 + y * (-0.109_862_862_7e-2 + y * (0.273_451_040_7e-4 + y * (-0.207_337_063_9e-5 + y * 0.209_388_721_1e-6)));
    let a2 =
        -0.156_249_999_5e-1 + y * (0.143_048_876_5e-3 + y * (-0.691_114_765_1e-5 + y * (0.762_109_516_1e-6 + y * (-0.934_945_152e-7))));
    sqrt(0.636_619_772 / x) * (xl_sin(xx) * a1 + z * xl_cos(xx) * a2)
}

fn bessy1(x: f64) -> f64 {
    if x < 8.0 {
        let y = x * x;
        let a1 = x
            * (-0.490_060_494_3e13
                + y * (0.127_527_439_0e13
                    + y * (-0.515_343_813_9e11 + y * (0.734_926_455_1e9 + y * (-0.423_792_272_6e7 + y * 0.851_193_793_5e4)))));
        let a2 = 0.249_958_057_0e14
            + y * (0.424_441_966_4e12
                + y * (0.373_365_036_7e10 + y * (0.224_590_400_2e8 + y * (0.102_042_605_0e6 + y * (0.354_963_288_5e3 + y)))));
        return a1 / a2 + 0.636_619_772 * (bessj1(x) * log(x) - 1.0 / x);
    }
    let z = 8.0 / x;
    let y = z * z;
    let xx = x - 2.356_194_491;
    let a1 = 1.0 + y * (0.183_105e-2 + y * (-0.351_639_649_6e-4 + y * (0.245_752_017_4e-5 + y * (-0.240_337_019e-6))));
    let a2 = 0.046_874_999_95 + y * (-0.200_269_087_3e-3 + y * (0.844_919_909_6e-5 + y * (-0.882_289_87e-6 + y * 0.105_787_412e-6)));
    sqrt(0.636_619_772 / x) * (xl_sin(xx) * a1 + z * xl_cos(xx) * a2)
}

/// Upward recurrence. Once the value is not finite it stays so (every step multiplies it by a positive factor and
/// subtracts a value): the loop stops there with the same non-finite result class (`#NUM!` after `finFlush`).
fn bessy(n: f64, x: f64) -> Option<f64> {
    if n == 0.0 {
        return Some(bessy0(x));
    }
    if n == 1.0 {
        return Some(bessy1(x));
    }
    let mut b = Budget(0);
    if over_budget(n - 1.0) {
        return None;
    }
    let tox = 2.0 / x;
    let mut by = bessy1(x);
    let mut bym = bessy0(x);
    let mut j = 1.0;
    while j < n {
        if !by.is_finite() {
            return Some(by);
        }
        b.step()?;
        let byp = j * tox * by - bym;
        bym = by;
        by = byp;
        j += 1.0;
    }
    Some(by)
}

fn bessi0(x: f64) -> f64 {
    let ax = abs(x);
    if ax < 3.75 {
        let mut y = x / 3.75;
        y *= y;
        return 1.0 + y * (3.515_622_9 + y * (3.089_942_4 + y * (1.206_749_2 + y * (0.265_973_2 + y * (0.360_768e-1 + y * 0.458_13e-2)))));
    }
    let y = 3.75 / ax;
    (exp(ax) / sqrt(ax))
        * (0.398_942_28
            + y * (0.132_859_2e-1
                + y * (0.225_319e-2
                    + y * (-0.157_565e-2
                        + y * (0.916_281e-2 + y * (-0.205_770_6e-1 + y * (0.263_553_7e-1 + y * (-0.164_763_3e-1 + y * 0.392_377e-2))))))))
}

fn bessi1(x: f64) -> f64 {
    let ax = abs(x);
    let mut ans;
    if ax < 3.75 {
        let mut y = x / 3.75;
        y *= y;
        ans = ax
            * (0.5
                + y * (0.878_905_94
                    + y * (0.514_988_69 + y * (0.150_849_34 + y * (0.265_873_3e-1 + y * (0.301_532e-2 + y * 0.324_11e-3))))));
    } else {
        let y = 3.75 / ax;
        ans = 0.228_296_7e-1 + y * (-0.289_531_2e-1 + y * (0.178_765_4e-1 - y * 0.420_059e-2));
        ans = 0.398_942_28 + y * (-0.398_802_4e-1 + y * (-0.362_018e-2 + y * (0.163_801e-2 + y * (-0.103_155_5e-1 + y * ans))));
        ans *= exp(ax) / sqrt(ax);
    }
    if x < 0.0 { -ans } else { ans }
}

fn bessi(n: f64, x: f64) -> Option<f64> {
    if n == 0.0 {
        return Some(bessi0(x));
    }
    if n == 1.0 {
        return Some(bessi1(x));
    }
    if x == 0.0 {
        return Some(0.0);
    }
    let mut b = Budget(0);
    let tox = 2.0 / abs(x);
    let (mut bip, mut ans, mut bi) = (0.0, 0.0, 1.0);
    let mut j = 2.0 * (n + trunc(sqrt(40.0 * n)));
    if over_budget(j) {
        return None;
    }
    while j > 0.0 {
        b.step()?;
        let bim = bip + j * tox * bi;
        bip = bi;
        bi = bim;
        if abs(bi) > 1e10 {
            ans *= 1e-10;
            bi *= 1e-10;
            bip *= 1e-10;
        }
        if j == n {
            ans = bip;
        }
        j -= 1.0;
    }
    ans *= bessi0(x) / bi;
    Some(if x < 0.0 && odd(n) { -ans } else { ans })
}

fn bessk0(x: f64) -> f64 {
    if x <= 2.0 {
        let y = (x * x) / 4.0;
        return -log(x / 2.0) * bessi0(x)
            + (-0.577_215_66
                + y * (0.422_784_20 + y * (0.230_697_56 + y * (0.348_859_0e-1 + y * (0.262_698e-2 + y * (0.107_50e-3 + y * 0.74e-5))))));
    }
    let y = 2.0 / x;
    (exp(-x) / sqrt(x))
        * (1.253_314_14
            + y * (-0.783_235_8e-1
                + y * (0.218_956_8e-1 + y * (-0.106_244_6e-1 + y * (0.587_872e-2 + y * (-0.251_540e-2 + y * 0.532_08e-3))))))
}

fn bessk1(x: f64) -> f64 {
    if x <= 2.0 {
        let y = (x * x) / 4.0;
        return log(x / 2.0) * bessi1(x)
            + (1.0 / x)
                * (1.0
                    + y * (0.154_431_44
                        + y * (-0.672_785_79 + y * (-0.181_568_97 + y * (-0.191_940_2e-1 + y * (-0.110_404e-2 + y * (-0.468_6e-4)))))));
    }
    let y = 2.0 / x;
    (exp(-x) / sqrt(x))
        * (1.253_314_14
            + y * (0.234_986_19
                + y * (-0.365_562_0e-1 + y * (0.150_426_8e-1 + y * (-0.780_353e-2 + y * (0.325_614e-2 + y * (-0.682_45e-3)))))))
}

/// Upward recurrence (same absorbing argument as [`bessy`]).
fn bessk(n: f64, x: f64) -> Option<f64> {
    if n == 0.0 {
        return Some(bessk0(x));
    }
    if n == 1.0 {
        return Some(bessk1(x));
    }
    let mut b = Budget(0);
    if over_budget(n - 1.0) {
        return None;
    }
    let tox = 2.0 / x;
    let mut bkm = bessk0(x);
    let mut bk = bessk1(x);
    let mut j = 1.0;
    while j < n {
        if !bk.is_finite() {
            return Some(bk);
        }
        b.step()?;
        let bkp = bkm + j * tox * bk;
        bkm = bk;
        bk = bkp;
        j += 1.0;
    }
    Some(bk)
}

/// `besselFn(name, f, positiveX)`.
fn bessel(a: &CallArgs<'_>, ctx: &mut dyn FnContext, g: fn(f64, f64) -> Option<f64>, positive_x: bool) -> R<Value> {
    atp_fn(a, ctx, &NumSpec::new(2, 2), |xs, _| {
        let (x, nn) = (xs.at(0), xs.at(1));
        if nn < 0.0 {
            return Ok(err(ErrorCode::Num)); // measured: BESSELI(1,5;−0,5) = #NUM! (before truncating)
        }
        let k = trunc(nn);
        if positive_x && x <= 0.0 {
            return Ok(err(ErrorCode::Num));
        }
        Ok(match g(k, x).map(fin_flush) {
            Some(Ok(v)) => num(v),
            Some(Err(e)) => err(e),
            None => err(ErrorCode::Num),
        })
    })
}

fn besselj(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    bessel(a, c, bessj, false)
}
fn bessely(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    bessel(a, c, bessy, true)
}
fn besseli(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    bessel(a, c, bessi, false)
}
fn besselk(a: &CallArgs<'_>, c: &mut dyn FnContext) -> R<Value> {
    bessel(a, c, bessk, true)
}
