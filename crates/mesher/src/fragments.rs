//! Circle tessellation settings (`$fn`, `$fa`, `$fs`) and trigonometry in
//! degrees with OpenSCAD's exact values at special angles.

use geometry::csg::Segments;

/// Radii below this produce the minimum fragment count (2^-20, OpenSCAD's
/// fine grid).
const GRID_FINE: f64 = 1.0 / 1_048_576.0;

/// Number of fragments for a full circle of radius `r`, following the
/// formula in the OpenSCAD manual: `$fn` when positive (at least 3),
/// otherwise `ceil(max(min(360 / $fa, r * 2 * PI / $fs), 5))`.
pub fn fragments(r: f64, seg: &Segments) -> u32 {
    if r.is_nan() || r < GRID_FINE || !r.is_finite() {
        return 3;
    }
    if seg.fn_ > 0.0 {
        return if seg.fn_ >= 3.0 {
            clamp_count(seg.fn_.floor())
        } else {
            3
        };
    }
    let n = (360.0 / seg.fa)
        .min(r * 2.0 * std::f64::consts::PI / seg.fs)
        .max(5.0)
        .ceil();
    clamp_count(n)
}

/// Converts a (possibly huge or NaN) count to `u32`, saturating at a sane
/// limit so that absurd `$fn` values cannot exhaust memory.
pub fn clamp_count(n: f64) -> u32 {
    const MAX: f64 = 1_000_000.0;
    if n.is_nan() {
        3
    } else {
        n.clamp(0.0, MAX) as u32
    }
}

// Trigonometry in degrees, matching OpenSCAD's vertex coordinates bit for
// bit (fitted against its STL output): the angle is reduced to the first
// octant by symmetry, exact values are used at 0°, 30°, 45°, 60° and 90°,
// and otherwise the correctly rounded sine or cosine of the angle is
// taken (the platform `sin` is off by one ulp too often); the radians are
// `a * (PI / 180)`.

/// Reduces `x` to `[0, 90]`; returns the angle and the signs of its sine
/// and cosine.
fn reduce(x: f64) -> (f64, bool, bool) {
    let mut a = x % 360.0;
    if a < 0.0 {
        a += 360.0;
    }
    let (mut sin_neg, mut cos_neg) = (false, false);
    if a >= 180.0 {
        a -= 180.0;
        sin_neg = true;
        cos_neg = true;
    }
    if a > 90.0 {
        a = 180.0 - a;
        cos_neg = !cos_neg;
    }
    (a, sin_neg, cos_neg)
}

/// Sine of an angle in `[0, 90]` degrees.
fn sin_first_quadrant(a: f64) -> f64 {
    match a {
        0.0 => 0.0,
        30.0 => 0.5,
        45.0 => std::f64::consts::FRAC_1_SQRT_2,
        60.0 => 0.866_025_403_784_438_6,
        90.0 => 1.0,
        _ if a > 45.0 => dd::cos((90.0 - a) * (std::f64::consts::PI / 180.0)),
        _ => dd::sin(a * (std::f64::consts::PI / 180.0)),
    }
}

fn signed(v: f64, negative: bool) -> f64 {
    if negative { -v } else { v }
}

/// Sine in degrees, as OpenSCAD computes vertex coordinates.
pub fn sin_degrees(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    let (a, sin_neg, _) = reduce(x);
    signed(sin_first_quadrant(a), sin_neg)
}

/// Cosine in degrees, as OpenSCAD computes vertex coordinates.
pub fn cos_degrees(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    let (a, _, cos_neg) = reduce(x);
    signed(sin_first_quadrant(90.0 - a), cos_neg)
}

/// Correctly rounded sine and cosine for `|x| <= PI / 4`, evaluated in
/// double-double arithmetic.
mod dd {
    #[derive(Clone, Copy)]
    struct Dd(f64, f64);

    fn two_sum(a: f64, b: f64) -> Dd {
        let s = a + b;
        let bb = s - a;
        Dd(s, (a - (s - bb)) + (b - bb))
    }

    fn add(a: Dd, b: Dd) -> Dd {
        let s = two_sum(a.0, b.0);
        let t = two_sum(a.1, b.1);
        let s = two_sum(s.0, s.1 + t.0);
        two_sum(s.0, s.1 + t.1)
    }

    fn mul(a: Dd, b: Dd) -> Dd {
        let p = a.0 * b.0;
        let e = a.0.mul_add(b.0, -p);
        two_sum(p, e + (a.0 * b.1 + a.1 * b.0))
    }

    fn div_int(a: Dd, n: f64) -> Dd {
        let q1 = a.0 / n;
        let p = q1 * n;
        let e = q1.mul_add(n, -p);
        let r = add(a, Dd(-p, -e));
        let q2 = r.0 / n;
        two_sum(q1, q2)
    }

    /// Sum of the Taylor series `sum (-1)^k x^(2k + first) / (2k + first)!`.
    fn series(x: f64, first: u32) -> f64 {
        let x = Dd(x, 0.0);
        let x2 = mul(x, x);
        let mut term = if first == 1 { x } else { Dd(1.0, 0.0) };
        let mut sum = term;
        for k in 1..16u32 {
            let n = 2 * k + first - 1;
            term = div_int(mul(term, x2), -f64::from(n * (n + 1)));
            sum = add(sum, term);
            if term.0.abs() < 1e-40 {
                break;
            }
        }
        sum.0 + sum.1
    }

    pub fn sin(x: f64) -> f64 {
        series(x, 1)
    }

    pub fn cos(x: f64) -> f64 {
        series(x, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(fn_: f64, fa: f64, fs: f64) -> Segments {
        Segments { fn_, fa, fs }
    }

    #[test]
    fn manual_formula() {
        let default = Segments::default();
        // r = 1: 2 * PI / 2 = 3.14 -> clamped to 5.
        assert_eq!(fragments(1.0, &default), 5);
        // r = 10: min(30, 31.4) = 30.
        assert_eq!(fragments(10.0, &default), 30);
        assert_eq!(fragments(5.0, &default), 16);
        assert_eq!(fragments(1.0, &seg(8.0, 12.0, 2.0)), 8);
        assert_eq!(fragments(1.0, &seg(2.0, 12.0, 2.0)), 3);
        assert_eq!(fragments(1.0, &seg(7.9, 12.0, 2.0)), 7);
        assert_eq!(fragments(1.0, &seg(0.0, 1.0, 0.1)), 63);
        // Tiny radii.
        assert_eq!(fragments(0.0, &default), 3);
        assert_eq!(fragments(1e-7, &seg(32.0, 12.0, 2.0)), 3);
    }

    #[test]
    fn exact_angles() {
        assert_eq!(sin_degrees(30.0), 0.5);
        assert_eq!(cos_degrees(60.0), 0.5);
        assert_eq!(cos_degrees(90.0), 0.0);
        assert_eq!(sin_degrees(180.0).to_bits(), (-0.0f64).to_bits());
    }
}
