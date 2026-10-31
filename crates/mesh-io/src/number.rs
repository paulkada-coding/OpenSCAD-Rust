//! Number formatting used by the text mesh formats.

/// Formats `v` like OpenSCAD's ASCII STL export: the shortest decimal string
/// that reads back to the same `f64`, in plain notation when the decimal
/// exponent is in `-7 < e < 21` and in exponent notation otherwise
/// (`1e-7`, `1.5e21`, no `+` sign). `-0` prints as `0`.
///
/// Observed: `0.000001`, `1e-7`, `110000000000000000000`, `1e21`,
/// `0.000012345678901234568`, `-9.999671966135733e-29`.
pub fn format_shortest(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf" } else { "-inf" }.into();
    }
    // `{:e}` gives the shortest round-trip digits: "-1.2345e-7", "1e22".
    let sci = format!("{v:e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let k = digits.len() as i32;
    // value = 0.d1d2...dk * 10^n
    let n = exp + 1;
    let mut out = String::with_capacity(k as usize + 8);
    if negative {
        out.push('-');
    }
    if k <= n && n <= 21 {
        out.push_str(&digits);
        out.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        out.push_str(&digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push_str(&(n - 1).to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::format_shortest as f;

    #[test]
    fn matches_observed_openscad_output() {
        assert_eq!(f(0.0), "0");
        assert_eq!(f(-0.0), "0");
        assert_eq!(f(10.0), "10");
        assert_eq!(f(-5.0), "-5");
        assert_eq!(f(0.000001), "0.000001");
        assert_eq!(f(1e-7), "1e-7");
        assert_eq!(f(-9e-9), "-9e-9");
        assert_eq!(f(1.1e20), "110000000000000000000");
        assert_eq!(f(1e21), "1e21");
        assert_eq!(f(1.1e22), "1.1e22");
        assert_eq!(f(1.2345678901234567e-5), "0.000012345678901234568");
        assert_eq!(f(-9.999671966135733e-29), "-9.999671966135733e-29");
        assert_eq!(f(0.9011997775086847), "0.9011997775086847");
        assert_eq!(f(4.903926402016152), "4.903926402016152");
        assert_eq!(f(-4.621477693630208), "-4.621477693630208");
    }

    #[test]
    fn round_trips() {
        for v in [1.0 / 3.0, -2.5e-12, 123456.789, 6.02e23, 1e-300, f64::MAX] {
            assert_eq!(f(v).parse::<f64>().unwrap(), v);
        }
    }
}
