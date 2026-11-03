//! Only the six constants that exist in JavaScript may be used from `core::f64::consts`; the
//! others (`TAU`, `FRAC_PI_3`, `LOG2_E`, ...) are correctly rounded values where the TS computes
//! an expression (`Math.PI / 3`). The bits below are `Math.PI`, `Math.E`, `Math.LN2`,
//! `Math.LN10`, `Math.SQRT2` and `Math.SQRT1_2` of Node 24.15.0.

use core::f64::consts;

#[test]
fn the_six_js_constants_have_the_bits_of_the_math_object() {
    assert_eq!(consts::PI.to_bits(), 0x4009_21FB_5444_2D18);
    assert_eq!(consts::E.to_bits(), 0x4005_BF0A_8B14_5769);
    assert_eq!(consts::LN_2.to_bits(), 0x3FE6_2E42_FEFA_39EF);
    assert_eq!(consts::LN_10.to_bits(), 0x4002_6BB1_BBB5_5516);
    assert_eq!(consts::SQRT_2.to_bits(), 0x3FF6_A09E_667F_3BCD);
    assert_eq!(consts::FRAC_1_SQRT_2.to_bits(), 0x3FE6_A09E_667F_3BCD);
}
