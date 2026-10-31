//! Number formatting compatible with OpenSCAD.
//!
//! Both styles round the exact binary value to 6 significant digits, but
//! they differ in details (observed with OpenSCAD 2026.10):
//!
//! | | `echo` / `str()` | CSG export (C `%g`) |
//! |---|---|---|
//! | exact ties | away from zero | to even |
//! | fixed notation for exponents | -5 ..= 5 | -4 ..= 5 |
//! | exponent | `1e+6` | `1e+06` |
//! | negative zero | `0` | `-0` |

const PRECISION: usize = 6;

#[derive(Clone, Copy)]
struct Style {
    ties_to_even: bool,
    min_fixed_exponent: i32,
    exponent_digits: usize,
    negative_zero: bool,
}

const ECHO: Style = Style {
    ties_to_even: false,
    min_fixed_exponent: -5,
    exponent_digits: 1,
    negative_zero: false,
};

const CSG: Style = Style {
    ties_to_even: true,
    min_fixed_exponent: -4,
    exponent_digits: 2,
    negative_zero: true,
};

/// Number as printed by `echo` and `str()`.
pub fn format_number(n: f64) -> String {
    format_with(n, ECHO)
}

/// Number as printed in OpenSCAD's `.csg` export.
pub fn format_number_csg(n: f64) -> String {
    format_with(n, CSG)
}

fn format_with(n: f64, style: Style) -> String {
    if n.is_nan() {
        return "nan".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if n == 0.0 {
        return if style.negative_zero && n.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }

    let (digits, exponent) = round_significant(n.abs(), style.ties_to_even);
    let sign = if n < 0.0 { "-" } else { "" };

    if (style.min_fixed_exponent..PRECISION as i32).contains(&exponent) {
        let mut text = String::new();
        if exponent < 0 {
            text.push_str("0.");
            text.extend(std::iter::repeat_n('0', (-exponent - 1) as usize));
            text.push_str(&digits);
        } else {
            let int_len = exponent as usize + 1;
            text.push_str(&digits[..int_len]);
            text.push('.');
            text.push_str(&digits[int_len..]);
        }
        format!("{sign}{}", trim_fraction(text))
    } else {
        let mantissa = trim_fraction(format!("{}.{}", &digits[..1], &digits[1..]));
        let exp_sign = if exponent < 0 { '-' } else { '+' };
        format!(
            "{sign}{mantissa}e{exp_sign}{:0width$}",
            exponent.abs(),
            width = style.exponent_digits
        )
    }
}

/// Rounds a positive finite number to `PRECISION` significant digits.
/// Returns the digits and the decimal exponent of the first one.
fn round_significant(n: f64, ties_to_even: bool) -> (String, i32) {
    // Rust prints the exact decimal expansion for large precisions; 40
    // extra digits are enough to recognize exact ties.
    let exact = format!("{n:.40e}");
    let (mantissa, exponent) = exact.split_once('e').expect("exponent format");
    let mut exponent: i32 = exponent.parse().expect("integer exponent");
    let all: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();

    let mut kept = all[..PRECISION].to_vec();
    let rest = &all[PRECISION..];
    let round_up = match rest[0].cmp(&5) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => {
            let exact_tie = rest[1..].iter().all(|&d| d == 0);
            if !exact_tie {
                true
            } else if ties_to_even {
                kept[PRECISION - 1] % 2 == 1
            } else {
                true
            }
        }
    };
    if round_up {
        let mut i = PRECISION;
        loop {
            if i == 0 {
                // 999999 → 1000000: one more digit before the point.
                kept.insert(0, 1);
                kept.truncate(PRECISION);
                exponent += 1;
                break;
            }
            i -= 1;
            if kept[i] == 9 {
                kept[i] = 0;
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    let digits = kept.iter().map(|d| char::from(b'0' + d)).collect();
    (digits, exponent)
}

fn trim_fraction(mut text: String) -> String {
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{format_number as echo, format_number_csg as csg};

    #[test]
    fn echo_style() {
        let cases: &[(f64, &str)] = &[
            (1.0, "1"),
            (-2.25, "-2.25"),
            (1.0 / 3.0, "0.333333"),
            (0.1 + 0.2, "0.3"),
            (123456.0, "123456"),
            (1234567.0, "1.23457e+6"),
            (1e20, "1e+20"),
            (0.00001, "0.00001"),
            (0.0000123456, "0.0000123456"),
            (1e-6, "1e-6"),
            (1234565.0, "1.23457e+6"),
            (123456.5, "123457"),
            (999999.5, "1e+6"),
            (-0.0, "0"),
            (2f64.powi(53), "9.0072e+15"),
            (5e-324, "4.94066e-324"),
        ];
        for &(n, expected) in cases {
            assert_eq!(echo(n), expected, "{n:e}");
        }
    }

    #[test]
    fn csg_style() {
        let cases: &[(f64, &str)] = &[
            (0.00001, "1e-05"),
            (0.000015, "1.5e-05"),
            (0.0001, "0.0001"),
            (1e6, "1e+06"),
            (1234565.0, "1.23456e+06"),
            (1234585.0, "1.23458e+06"),
            (123456.5, "123456"),
            (999999.5, "1e+06"),
            (-0.0, "-0"),
            (1e100, "1e+100"),
            (1.0 / 3.0, "0.333333"),
        ];
        for &(n, expected) in cases {
            assert_eq!(csg(n), expected, "{n:e}");
        }
    }
}
