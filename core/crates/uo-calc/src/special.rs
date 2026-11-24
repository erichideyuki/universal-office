//! Special functions of the distributions (`special-functions.ts`): normal (Cody), inverse normal (Wichura AS241),
//! gamma and log-gamma, regularized incomplete gamma (R's `pgamma`, Welinder), incomplete beta (Lentz), Loader's
//! binomial/Poisson densities (`stirlerr`, `bd0`). Same operations in the same order as the TS, `Math.*` from
//! `uo_jsnum` (V8's fdlibm).
//!
//! **Declared (A1-3)**: the series and continued fractions whose TS loop has no explicit bound (`logcf`, the small-x
//! series of `pgamma`, `pdUpperSeries`, `pdLowerSeries`) stop after [`LOOP_CAP`] iterations. The branch conditions
//! of `pgamma` keep their ratios ≤ 0.8 (and x < 1 for the small-x series), so they converge in a few hundred steps
//! for every finite input and the cap never binds there; it only guarantees that no input, however hostile, can keep
//! the tab busy (the result past the cap would be the partial sum).
#![expect(clippy::neg_cmp_op_on_partial_ord, reason = "the TS `!(x > y)` tests: NaN takes the negated branch")]
#![expect(clippy::excessive_precision, reason = "the TS literals, digit for digit")]
#![expect(clippy::approx_constant, reason = "the TS literals, digit for digit")]

use uo_jsnum::math::{exp, expm1, floor, log, log1p, max, min, sin, sqrt, trunc};

/// Iteration cap of the unbounded TS loops (declared A1-3).
pub const LOOP_CAP: u32 = 1 << 26;

const SQRT_32: f64 = 5.656854249492380195206754896838;
const M_1_SQRT_2PI: f64 = 0.398942280401432677939946059934;
/// `ln √(2π)`.
pub const LN_SQRT_2PI: f64 = 0.918938533204672741780329736406;
const DBL_EPS: f64 = 2.220446049250313e-16;
/// `Number.MIN_VALUE`.
const MIN_VALUE: f64 = 5e-324;
const PI: f64 = core::f64::consts::PI;

/// `pnormBoth(x)`: `(Φ(x), 1 − Φ(x))` of the standard normal (Cody, as R's `pnorm`).
#[must_use]
pub fn pnorm_both(x: f64) -> (f64, f64) {
    const A: [f64; 5] =
        [2.2352520354606839287, 161.02823106855587881, 1067.6894854603709582, 18154.981253343561249, 0.065682337918207449113];
    const B: [f64; 4] = [47.20258190468824187, 976.09855173777669322, 10260.932208618978205, 45507.789335026729956];
    const C: [f64; 9] = [
        0.39894151208813466764,
        8.8831497943883759412,
        93.506656132177855979,
        597.27027639480026226,
        2494.5375852903726711,
        6848.1904505362823326,
        11602.651437647350124,
        9842.7148383839780218,
        1.0765576773720192317e-8,
    ];
    const D: [f64; 8] = [
        22.266688044328115691,
        235.38790178262499861,
        1519.377599407554805,
        6485.558298266760755,
        18615.571640885098091,
        34900.952721145977266,
        38912.003286093271411,
        19685.429676859990727,
    ];
    const P: [f64; 6] = [
        0.21589853405795699,
        0.1274011611602473639,
        0.022235277870649807,
        0.001421619193227893466,
        2.9112874951168792e-5,
        0.02307344176494017303,
    ];
    const Q: [f64; 5] = [1.28426009614491121, 0.468238212480865118, 0.0659881378689285515, 0.00378239633202758244, 7.29751555083966205e-5];
    if x.is_nan() {
        return (f64::NAN, f64::NAN);
    }
    let y = x.abs();
    if y <= 0.67448975 {
        let (mut xnum, mut xden) = (0.0, 0.0);
        if y > DBL_EPS * 0.5 {
            let xsq = x * x;
            xnum = A[4] * xsq;
            xden = xsq;
            for i in 0..3 {
                xnum = (xnum + A[i]) * xsq;
                xden = (xden + B[i]) * xsq;
            }
        }
        let temp = x * (xnum + A[3]) / (xden + B[3]);
        return (0.5 + temp, 0.5 - temp);
    }
    let del = |xx: f64, temp: f64| -> (f64, f64) {
        let xsq = trunc(xx * 16.0) / 16.0;
        let dl = (xx - xsq) * (xx + xsq);
        let cum = exp(-xsq * xsq * 0.5) * exp(-dl * 0.5) * temp;
        (cum, 1.0 - cum)
    };
    let (cum, ccum) = if y <= SQRT_32 {
        let mut xnum = C[8] * y;
        let mut xden = y;
        for i in 0..7 {
            xnum = (xnum + C[i]) * y;
            xden = (xden + D[i]) * y;
        }
        let temp = (xnum + C[7]) / (xden + D[7]);
        del(y, temp)
    } else if y < 50.0 {
        let xsq = 1.0 / (x * x);
        let mut xnum = P[5] * xsq;
        let mut xden = xsq;
        for i in 0..4 {
            xnum = (xnum + P[i]) * xsq;
            xden = (xden + Q[i]) * xsq;
        }
        let mut temp = xsq * (xnum + P[4]) / (xden + Q[4]);
        temp = (M_1_SQRT_2PI - temp) / y;
        del(x, temp)
    } else {
        (0.0, 1.0)
    };
    if x > 0.0 { (ccum, cum) } else { (cum, ccum) }
}

/// `pnorm(x)` = Φ(x).
#[must_use]
pub fn pnorm(x: f64) -> f64 {
    pnorm_both(x).0
}

/// `dnorm(x)`: the standard normal density.
#[must_use]
pub fn dnorm(x: f64) -> f64 {
    M_1_SQRT_2PI * exp(-0.5 * x * x)
}

/// `qnorm(p)` = Φ⁻¹(p), Wichura AS241 (PPND16).
#[must_use]
pub fn qnorm(p: f64) -> f64 {
    if !(p > 0.0 && p < 1.0) {
        return if p == 0.0 {
            f64::NEG_INFINITY
        } else if p == 1.0 {
            f64::INFINITY
        } else {
            f64::NAN
        };
    }
    let q = p - 0.5;
    if q.abs() <= 0.425 {
        let r = 0.180625 - q * q;
        return q
            * (((((((r * 2509.0809287301226727 + 33430.575583588128105) * r + 67265.770927008700853) * r + 45921.953931549871457) * r
                + 13731.693765509461125)
                * r
                + 1971.5909503065514427)
                * r
                + 133.14166789178437745)
                * r
                + 3.387132872796366608)
            / (((((((r * 5226.495278852545925 + 28729.085735721942674) * r + 39307.89580009271061) * r + 21213.794301586595867) * r
                + 5394.1960214247511077)
                * r
                + 687.1870074920579083)
                * r
                + 42.313330701600911252)
                * r
                + 1.0);
    }
    let mut r = if q < 0.0 { p } else { 1.0 - p };
    r = sqrt(-log(r));
    let val = if r <= 5.0 {
        r -= 1.6;
        (((((((r * 7.7454501427834140764e-4 + 0.0227238449892691845833) * r + 0.24178072517745061177) * r + 1.27045825245236838258) * r
            + 3.64784832476320460504)
            * r
            + 5.7694972214606914055)
            * r
            + 4.6303378461565452959)
            * r
            + 1.42343711074968357734)
            / (((((((r * 1.05075007164441684324e-9 + 5.475938084995344946e-4) * r + 0.0151986665636164571966) * r
                + 0.14810397642748007459)
                * r
                + 0.68976733498510000455)
                * r
                + 1.6763848301838038494)
                * r
                + 2.05319162663775882187)
                * r
                + 1.0)
    } else {
        r -= 5.0;
        (((((((r * 2.01033439929228813265e-7 + 2.71155556874348757815e-5) * r + 0.0012426609473880784386) * r + 0.026532189526576123093)
            * r
            + 0.29656057182850489123)
            * r
            + 1.7848265399172913358)
            * r
            + 5.4637849111641143699)
            * r
            + 6.6579046435011037772)
            / (((((((r * 2.04426310338993978564e-15 + 1.4215117583164458887e-7) * r + 1.8463183175100546818e-5) * r
                + 7.868691311456132591e-4)
                * r
                + 0.0148753612908506148525)
                * r
                + 0.13692988092273580531)
                * r
                + 0.59983220655588793769)
                * r
                + 1.0)
    };
    if q < 0.0 { -val } else { val }
}

// ---------------------------------------------------------------- gamma

/// Coefficients of `ln Γ(2+z) = (1−γ)z + Σ c_k z^k`, k = 2..=46.
const LG2_COEF: [f64; 45] = [
    0.3224670334241132,
    -0.067352301053198102,
    0.020580808427784546,
    -0.0073855510286739857,
    0.0028905103307415234,
    -0.001192753911703261,
    0.00050966952474304245,
    -0.00022315475845357939,
    0.000099457512781808531,
    -0.000044926236738133142,
    0.000020507212775670691,
    -0.0000094394882752683967,
    0.0000043748667899074882,
    -0.0000020392157538013662,
    9.5514121304074194e-7,
    -4.4924691987645662e-7,
    2.1207184805554665e-7,
    -1.0043224823968099e-7,
    4.7698101693639804e-8,
    -2.2711094608943164e-8,
    1.0838659214896955e-8,
    -5.1834750419700466e-9,
    2.4836745438024785e-9,
    -1.1921401405860912e-9,
    5.7313672416788623e-10,
    -2.7595228851242334e-10,
    1.3304764374244489e-10,
    -6.4229645638380996e-11,
    3.1044247747322276e-11,
    -1.5021384080754142e-11,
    7.2759744802390792e-12,
    -3.5277424765759151e-12,
    1.711991790559618e-12,
    -8.315385841420285e-13,
    4.0422005252894402e-13,
    -1.9664756310966165e-13,
    9.5736303878385556e-14,
    -4.6640760264283744e-14,
    2.2737369600659724e-14,
    -1.1091399470834522e-14,
    5.4136591567253633e-15,
    -2.6438800178609949e-15,
    1.2918959062789966e-15,
    -6.3159355041984482e-16,
    3.089316266963393e-16,
];
const EULER_GAMMA: f64 = 0.57721566490153286061;

/// `ln Γ(2+z)`, |z| ≤ 0.5.
fn lgamma2p(z: f64) -> f64 {
    let mut s = 0.0;
    for k in (0..LG2_COEF.len()).rev() {
        s = (s + LG2_COEF[k]) * z;
    }
    (s + (1.0 - EULER_GAMMA)) * z
}

/// `lgammacor(x)`: Stirling's correction `ln Γ(x) − ((x − ½)ln x − x + ln √(2π))`, x ≥ 10.
#[must_use]
pub fn lgammacor(x: f64) -> f64 {
    let x2 = 1.0 / (x * x);
    (1.0 / 12.0
        - x2 * (1.0 / 360.0 - x2 * (1.0 / 1260.0 - x2 * (1.0 / 1680.0 - x2 * (1.0 / 1188.0 - x2 * (691.0 / 360360.0 - x2 / 156.0))))))
        / x
}

/// `lgamma(x)`: `ln Γ(x)` for x > 0 (NaN otherwise).
#[must_use]
pub fn lgamma(x: f64) -> f64 {
    if !(x > 0.0) || !x.is_finite() {
        return f64::NAN;
    }
    if x < 0.5 {
        return lgamma2p(x) - log1p(x) - log(x);
    }
    if x < 1.5 {
        return lgamma2p(x - 1.0) - log1p(x - 1.0);
    }
    if x <= 2.5 {
        return lgamma2p(x - 2.0);
    }
    if x < 12.0 {
        let mut y = x;
        let mut prod = 1.0;
        while y > 2.5 {
            y -= 1.0;
            prod *= y;
        }
        return lgamma2p(y - 2.0) + log(prod);
    }
    (x - 0.5) * log(x) - x + LN_SQRT_2PI + lgammacor(x)
}

/// `gamma(x)`: Γ(x) for real x (poles → NaN).
#[must_use]
pub fn gamma(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    if x == floor(x) {
        if x <= 0.0 {
            return f64::NAN;
        }
        if x <= 171.0 {
            let mut f = 1.0;
            let mut i = 2.0;
            while i < x {
                f *= i;
                i += 1.0;
            }
            return f;
        }
        return f64::INFINITY;
    }
    if x < 0.5 {
        return PI / (sin(PI * x) * gamma(1.0 - x));
    }
    if x > 171.7 {
        return f64::INFINITY;
    }
    exp(lgamma(x))
}

// ---------------------------------------------------------------- erf / erfc (Cody CALERF, as Excel)

const ERF_A: [f64; 5] =
    [3.16112374387056560e00, 1.13864154151050156e02, 3.77485237685302021e02, 3.20937758913846947e03, 1.85777706184603153e-1];
const ERF_B: [f64; 4] = [2.36012909523441209e01, 2.44024637934444173e02, 1.28261652607737228e03, 2.84423683343917062e03];
const ERF_C: [f64; 9] = [
    5.64188496988670089e-1,
    8.88314979438837594e00,
    6.61191906371416295e01,
    2.98635138197400131e02,
    8.81952221241769090e02,
    1.71204761263407058e03,
    2.05107837782607147e03,
    1.23033935479799725e03,
    2.15311535474403846e-8,
];
const ERF_D: [f64; 8] = [
    1.57449261107098347e01,
    1.17693950891312499e02,
    5.37181101862009858e02,
    1.62138957456669019e03,
    3.29079923573345963e03,
    4.36261909014324716e03,
    3.43936767414372164e03,
    1.23033935480374942e03,
];
const ERF_P: [f64; 6] = [
    3.05326634961232344e-1,
    3.60344899949804439e-1,
    1.25781726111229246e-1,
    1.60837851487422766e-2,
    6.58749161529837803e-4,
    1.63153871373020978e-2,
];
const ERF_Q: [f64; 5] =
    [2.56852019228982242e00, 1.87295284992346725e00, 5.27905102951428412e-1, 6.05183413124413191e-2, 2.33520497626869185e-3];
const SQRPI: f64 = 5.6418958354775628695e-1;

/// CALERF: `erfc` when `complement`, else `erf`.
fn calerf(x: f64, complement: bool) -> f64 {
    let y = x.abs();
    let mut result;
    if y <= 0.46875 {
        let ysq = if y > 1.11e-16 { y * y } else { 0.0 };
        let mut xnum = ERF_A[4] * ysq;
        let mut xden = ysq;
        for i in 0..3 {
            xnum = (xnum + ERF_A[i]) * ysq;
            xden = (xden + ERF_B[i]) * ysq;
        }
        result = x * (xnum + ERF_A[3]) / (xden + ERF_B[3]);
        return if complement { 1.0 - result } else { result };
    }
    if y <= 4.0 {
        let mut xnum = ERF_C[8] * y;
        let mut xden = y;
        for i in 0..7 {
            xnum = (xnum + ERF_C[i]) * y;
            xden = (xden + ERF_D[i]) * y;
        }
        result = (xnum + ERF_C[7]) / (xden + ERF_D[7]);
        result *= exp(-y * y);
    } else if y >= 26.543 {
        result = 0.0;
    } else {
        let ysq = 1.0 / (y * y);
        let mut xnum = ERF_P[5] * ysq;
        let mut xden = ysq;
        for i in 0..4 {
            xnum = (xnum + ERF_P[i]) * ysq;
            xden = (xden + ERF_Q[i]) * ysq;
        }
        result = ysq * (xnum + ERF_P[4]) / (xden + ERF_Q[4]);
        result = (SQRPI - result) / y;
        result *= exp(-y * y);
    }
    if !complement {
        result = (0.5 - result) + 0.5;
        return if x < 0.0 { -result } else { result };
    }
    if x < 0.0 { 2.0 - result } else { result }
}

/// `erf(x)` (Cody).
#[must_use]
pub fn erf(x: f64) -> f64 {
    calerf(x, false)
}

/// `erfc(x)` (Cody).
#[must_use]
pub fn erfc(x: f64) -> f64 {
    calerf(x, true)
}

/// `normCdfXl(x)`: Φ(x) as Excel's `NORM.S.DIST`: `½·erfc(−x·(1/√2))`.
#[must_use]
pub fn norm_cdf_xl(x: f64) -> f64 {
    0.5 * erfc(-x * core::f64::consts::FRAC_1_SQRT_2)
}

// ---------------------------------------------------------------- Loader's densities

/// `stirlerr(n)`: `ln(n!) − ln(√(2πn)(n/e)^n)`.
#[must_use]
pub fn stirlerr(n: f64) -> f64 {
    const S0: f64 = 1.0 / 12.0;
    const S1: f64 = 1.0 / 360.0;
    const S2: f64 = 1.0 / 1260.0;
    const S3: f64 = 1.0 / 1680.0;
    const S4: f64 = 1.0 / 1188.0;
    const HALVES: [f64; 31] = [
        0.0,
        0.1534264097200273452913848,
        0.0810614667953272582196702,
        0.0548141210519176538961390,
        0.0413406959554092940938221,
        0.03316287351993628748511048,
        0.02767792568499833914878929,
        0.02374616365629749597132920,
        0.02079067210376509311152277,
        0.01848845053267318523077934,
        0.01664469118982119216319487,
        0.01513497322191737887351255,
        0.01387612882307074799874573,
        0.01281046524292022692424986,
        0.01189670994589177009505572,
        0.01110455975820691732662991,
        0.010411265261972096497478567,
        0.009799416126158803298389475,
        0.009255462182712732917728637,
        0.008768700134139385462952823,
        0.008330563433362871256469318,
        0.007934114564314020547248100,
        0.007573675487951840794972024,
        0.007244554301320383179543912,
        0.006942840107209529865664152,
        0.006665247032707682442354394,
        0.006408994188004207068439631,
        0.006171712263039457647532867,
        0.005951370112758847735624416,
        0.005746216513010115682023589,
        0.005554733551962801371038690,
    ];
    if n <= 15.0 {
        let nn = n + n;
        if nn == floor(nn) {
            // `sferr_halves[nn]!`: an index outside the table (negative or NaN-free non-integers excluded above) is
            // `undefined` in the TS, which arithmetic turns into NaN
            return uo_jsnum::conv::js_index(nn, HALVES.len()).map_or(f64::NAN, |i| HALVES[i]);
        }
        return lgamma(n + 1.0) - (n + 0.5) * log(n) + n - LN_SQRT_2PI;
    }
    let nn = n * n;
    if n > 500.0 {
        return (S0 - S1 / nn) / n;
    }
    if n > 80.0 {
        return (S0 - (S1 - S2 / nn) / nn) / n;
    }
    if n > 35.0 {
        return (S0 - (S1 - (S2 - S3 / nn) / nn) / nn) / n;
    }
    (S0 - (S1 - (S2 - (S3 - S4 / nn) / nn) / nn) / nn) / n
}

/// `bd0(x, np)` = `x ln(x/np) + np − x` without cancellation.
#[must_use]
pub fn bd0(x: f64, np: f64) -> f64 {
    if (x - np).abs() < 0.1 * (x + np) {
        let mut v = (x - np) / (x + np);
        let mut s = (x - np) * v;
        if s.abs() < MIN_VALUE {
            return s;
        }
        let mut ej = 2.0 * x * v;
        v *= v;
        for j in 1..1000 {
            ej *= v;
            let s1 = s + ej / f64::from((j << 1) + 1);
            if s1 == s {
                return s1;
            }
            s = s1;
        }
    }
    x * log(x / np) + np - x
}

/// `dbinomRaw(x, n, p, q)`: the binomial density P(X = x), q = 1 − p.
#[must_use]
pub fn dbinom_raw(x: f64, n: f64, p: f64, q: f64) -> f64 {
    if p == 0.0 {
        return if x == 0.0 { 1.0 } else { 0.0 };
    }
    if q == 0.0 {
        return if x == n { 1.0 } else { 0.0 };
    }
    if x == 0.0 {
        if n == 0.0 {
            return 1.0;
        }
        let lc = if p < 0.1 { -bd0(n, n * q) - n * p } else { n * log(q) };
        return exp(lc);
    }
    if x == n {
        let lc = if q < 0.1 { -bd0(n, n * p) - n * q } else { n * log(p) };
        return exp(lc);
    }
    if x < 0.0 || x > n {
        return 0.0;
    }
    let lc = stirlerr(n) - stirlerr(x) - stirlerr(n - x) - bd0(x, n * p) - bd0(n - x, n * q);
    // Loader's original form exp(lc)/√f
    exp(lc) / sqrt(2.0 * PI * x * (n - x) / n)
}

/// `dpoisRaw(x, λ)`: the Poisson density.
#[must_use]
pub fn dpois_raw(x: f64, lambda: f64) -> f64 {
    if lambda == 0.0 {
        return if x == 0.0 { 1.0 } else { 0.0 };
    }
    if !lambda.is_finite() || x < 0.0 {
        return 0.0;
    }
    if x <= lambda * MIN_VALUE {
        return exp(-lambda);
    }
    if lambda < x * MIN_VALUE {
        return exp(-lambda + x * log(lambda) - lgamma(x + 1.0));
    }
    exp(-stirlerr(x) - bd0(x, lambda)) / sqrt(2.0 * PI * x)
}

// ---------------------------------------------------------------- regularized incomplete gamma (R's pgamma)

const SCALEFACTOR: f64 = 1.157920892373162e77; // 2^256
const M_CUTOFF: f64 = core::f64::consts::LN_2 * 1024.0 / DBL_EPS;

/// `logcf(x, i, d, eps)`: continued fraction for `log1pmx` and `lgamma1p`.
fn logcf(x: f64, i: f64, d: f64, eps: f64) -> f64 {
    let mut c1 = 2.0 * d;
    let mut c2 = i + d;
    let mut c4 = c2 + d;
    let mut a1 = c2;
    let mut b1 = i * (c2 - i * x);
    let mut b2 = d * d * x;
    let mut a2 = c4 * c2 - b2;
    b2 = c4 * b1 - i * b2;
    let mut guard = 0u32;
    while (a2 * b1 - a1 * b2).abs() > (eps * b1 * b2).abs() && guard < LOOP_CAP {
        guard += 1;
        let mut c3 = c2 * c2 * x;
        c2 += d;
        c4 += d;
        a1 = c4 * a2 - c3 * a1;
        b1 = c4 * b2 - c3 * b1;
        c3 = c1 * c1 * x;
        c1 += d;
        c4 += d;
        a2 = c4 * a1 - c3 * a2;
        b2 = c4 * b1 - c3 * b2;
        if b2.abs() > SCALEFACTOR {
            a1 /= SCALEFACTOR;
            b1 /= SCALEFACTOR;
            a2 /= SCALEFACTOR;
            b2 /= SCALEFACTOR;
        } else if b2.abs() < 1.0 / SCALEFACTOR {
            a1 *= SCALEFACTOR;
            b1 *= SCALEFACTOR;
            a2 *= SCALEFACTOR;
            b2 *= SCALEFACTOR;
        }
    }
    a2 / b2
}

/// `log1pmx(x)` = `ln(1+x) − x` without cancellation.
#[must_use]
pub fn log1pmx(x: f64) -> f64 {
    if x > 1.0 || x < -0.79149064 {
        return log1p(x) - x;
    }
    let r = x / (2.0 + x);
    let y = r * r;
    if x.abs() < 1e-2 {
        return r * ((((2.0 / 9.0 * y + 2.0 / 7.0) * y + 2.0 / 5.0) * y + 2.0 / 3.0) * y - x);
    }
    r * (2.0 * y * logcf(y, 3.0, 2.0, 1e-14) - x)
}

/// `lgamma1p(a)` = `ln Γ(1+a)`, accurate also for |a| < 0.5.
#[must_use]
pub fn lgamma1p(a: f64) -> f64 {
    if a.abs() >= 0.5 {
        return lgamma(a + 1.0);
    }
    let c = 0.2273736845824652515226821577978691e-12; // ζ(42) − 1
    let mut lgam = c * logcf(-a / 2.0, 42.0, 1.0, 1e-14);
    for i in (0..=39).rev() {
        lgam = LG2_COEF[i].abs() - a * lgam;
    }
    (a * lgam - EULER_GAMMA) * a - log1pmx(a)
}

/// `dpois(x_plus_1 − 1, λ)` with the small-x cases.
fn dpois_wrap(x_plus1: f64, lambda: f64) -> f64 {
    if !lambda.is_finite() {
        return 0.0;
    }
    if x_plus1 > 1.0 {
        return dpois_raw(x_plus1 - 1.0, lambda);
    }
    if lambda > (x_plus1 - 1.0).abs() * M_CUTOFF {
        return exp(-lambda - lgamma(x_plus1));
    }
    dpois_raw(x_plus1, lambda) * (x_plus1 / lambda)
}

fn pgamma_smallx(x: f64, alph: f64, lower: bool) -> f64 {
    let mut sum = 0.0;
    let mut c = alph;
    let mut n = 0.0;
    let mut guard = 0u32;
    loop {
        n += 1.0;
        c *= -x / n;
        let term = c / (alph + n);
        sum += term;
        guard += 1;
        if !(term.abs() > DBL_EPS * sum.abs()) || guard >= LOOP_CAP {
            break;
        }
    }
    if lower {
        let f2 = if alph > 1.0 { dpois_raw(alph, x) * exp(x) } else { exp(alph * log(x) - lgamma1p(alph)) };
        return (1.0 + sum) * f2;
    }
    let lf2 = alph * log(x) - lgamma1p(alph);
    let f2m1 = expm1(lf2);
    -(sum + f2m1 + sum * f2m1)
}

fn pd_upper_series(x: f64, y: f64) -> f64 {
    let mut y = y;
    let mut term = x / y;
    let mut sum = term;
    let mut guard = 0u32;
    loop {
        y += 1.0;
        term *= x / y;
        sum += term;
        guard += 1;
        if !(term > sum * DBL_EPS) || guard >= LOOP_CAP {
            break;
        }
    }
    sum
}

fn pd_lower_cf(y: f64, d: f64) -> f64 {
    if y == 0.0 {
        return 0.0;
    }
    let mut f0 = y / d;
    if (y - 1.0).abs() < d.abs() * DBL_EPS {
        return f0;
    }
    if f0 > 1.0 {
        f0 = 1.0;
    }
    let mut c2 = y;
    let mut c4 = d;
    let mut a1 = 0.0;
    let mut b1 = 1.0;
    let mut a2 = y;
    let mut b2 = d;
    let mut f = 0.0;
    let mut of = -1.0;
    let mut i = 0.0;
    while b2 > SCALEFACTOR {
        a1 /= SCALEFACTOR;
        b1 /= SCALEFACTOR;
        a2 /= SCALEFACTOR;
        b2 /= SCALEFACTOR;
    }
    while i < 200000.0 {
        i += 1.0;
        c2 -= 1.0;
        let mut c3 = i * c2;
        c4 += 2.0;
        a1 = c4 * a2 + c3 * a1;
        b1 = c4 * b2 + c3 * b1;
        i += 1.0;
        c2 -= 1.0;
        c3 = i * c2;
        c4 += 2.0;
        a2 = c4 * a1 + c3 * a2;
        b2 = c4 * b1 + c3 * b2;
        if b2 > SCALEFACTOR {
            a1 /= SCALEFACTOR;
            b1 /= SCALEFACTOR;
            a2 /= SCALEFACTOR;
            b2 /= SCALEFACTOR;
        }
        if b2 != 0.0 {
            f = a2 / b2;
            if (f - of).abs() <= DBL_EPS * max(f0, f.abs()) {
                return f;
            }
            of = f;
        }
    }
    f
}

fn pd_lower_series(lambda: f64, y: f64) -> f64 {
    let mut y = y;
    let mut term = 1.0;
    let mut sum = 0.0;
    let mut guard = 0u32;
    while y >= 1.0 && term > sum * DBL_EPS && guard < LOOP_CAP {
        guard += 1;
        term *= y / lambda;
        sum += term;
        y -= 1.0;
    }
    if y != floor(y) {
        sum += term * pd_lower_cf(y, lambda + 1.0 - y);
    }
    sum
}

fn ppois_asymp(x: f64, lambda: f64, lower: bool) -> f64 {
    const CA: [f64; 8] =
        [0.0, 2.0 / 3.0, -4.0 / 135.0, 8.0 / 2835.0, 16.0 / 8505.0, -8992.0 / 12629925.0, -334144.0 / 492567075.0, 698752.0 / 1477701225.0];
    const CB: [f64; 8] = [
        0.0,
        1.0 / 12.0,
        1.0 / 288.0,
        -139.0 / 51840.0,
        -571.0 / 2488320.0,
        163879.0 / 209018880.0,
        5246819.0 / 75246796800.0,
        -534703531.0 / 902961561600.0,
    ];
    let dfm = lambda - x;
    let pt = -log1pmx(dfm / x);
    let mut s2pt = sqrt(2.0 * x * pt);
    if dfm < 0.0 {
        s2pt = -s2pt;
    }
    let mut res12 = 0.0;
    let mut res1_term = sqrt(x);
    let mut res1_ig = res1_term;
    let mut res2_term = s2pt;
    let mut res2_ig = s2pt;
    for i in 1..8 {
        res12 += res1_ig * CA[i] + res2_ig * CB[i];
        res1_term *= pt / i as f64;
        res2_term *= 2.0 * pt / (2 * i + 1) as f64;
        res1_ig = res1_ig / x + res1_term;
        res2_ig = res2_ig / x + res2_term;
    }
    let mut elfb = x;
    let mut elfb_term = 1.0;
    for cb in CB.iter().skip(1) {
        elfb += elfb_term * cb;
        elfb_term /= x;
    }
    if !lower {
        elfb = -elfb;
    }
    let f = res12 / elfb;
    let (cum, ccum) = pnorm_both(s2pt);
    (if lower { ccum } else { cum }) + f * dnorm(s2pt)
}

/// `pgamma(x, alph, lower)`: P(a, x) (`lower`) or Q(a, x) of the regularized gamma.
#[must_use]
pub fn pgamma(x: f64, alph: f64, lower: bool) -> f64 {
    if x <= 0.0 {
        return if lower { 0.0 } else { 1.0 };
    }
    if !x.is_finite() {
        return if lower { 1.0 } else { 0.0 };
    }
    if x < 1.0 {
        return pgamma_smallx(x, alph, lower);
    }
    if x <= alph - 1.0 && x < 0.8 * (alph + 50.0) {
        let sum = pd_upper_series(x, alph);
        let d = dpois_wrap(alph, x);
        return if lower { sum * d } else { 1.0 - d * sum };
    }
    if alph - 1.0 < x && alph < 0.8 * (x + 50.0) {
        let d = dpois_wrap(alph, x);
        let sum = if alph < 1.0 {
            if x * DBL_EPS > 1.0 - alph { 1.0 } else { pd_lower_cf(alph, x - (alph - 1.0)) * x / alph }
        } else {
            1.0 + pd_lower_series(x, alph - 1.0)
        };
        return if lower { 1.0 - d * sum } else { sum * d };
    }
    ppois_asymp(alph - 1.0, x, !lower)
}

/// `gammaInc(a, x)`: `(P(a, x), Q(a, x))`.
#[must_use]
pub fn gamma_inc(a: f64, x: f64) -> (f64, f64) {
    (pgamma(x, a, true), pgamma(x, a, false))
}

// ---------------------------------------------------------------- beta

/// `lbeta(a, b)` = `ln B(a, b)`.
#[must_use]
pub fn lbeta(a: f64, b: f64) -> f64 {
    let p = min(a, b);
    let q = max(a, b);
    if p >= 10.0 {
        let corr = lgammacor(p) + lgammacor(q) - lgammacor(p + q);
        return log(q) * -0.5 + LN_SQRT_2PI + corr + (p - 0.5) * log(p / (p + q)) + q * log1p(-p / (p + q));
    }
    if q >= 10.0 {
        let corr = lgammacor(q) - lgammacor(p + q);
        return lgamma(p) + corr + p - p * log(p + q) + (q - 0.5) * log1p(-p / (p + q));
    }
    lgamma(p) + lgamma(q) - lgamma(p + q)
}

/// `betaInc(x, a, b)`: `(I_x(a, b), 1 − I_x(a, b))`.
#[must_use]
pub fn beta_inc(x: f64, a: f64, b: f64) -> (f64, f64) {
    if x <= 0.0 {
        return (0.0, 1.0);
    }
    if x >= 1.0 {
        return (1.0, 0.0);
    }
    let y = 1.0 - x;
    if x < (a + 1.0) / (a + b + 2.0) {
        let p = b / (a + b) * dbinom_raw(a, a + b, x, y) * betacf(x, a, b);
        return (p, 1.0 - p);
    }
    let q = a / (a + b) * dbinom_raw(b, a + b, y, x) * betacf(y, b, a);
    (1.0 - q, q)
}

/// Continued fraction of the incomplete beta (Lentz).
fn betacf(x: f64, a: f64, b: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for mi in 1..=100_000u32 {
        let m = f64::from(mi);
        let m2 = 2.0 * m;
        let mut aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        h *= d * c;
        aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < 1e-16 {
            break;
        }
    }
    h
}
