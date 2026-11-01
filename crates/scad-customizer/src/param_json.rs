//! Serialization of extracted parameters into OpenSCAD's `.param` JSON format.

use crate::{NumberOption, ParamConstraint, ParamValue, Parameter, StringOption};

/// Formats extracted parameters into OpenSCAD's compact `.param` JSON string
/// (alphabetically sorted object keys, no trailing newline, `"parameters"`
/// omitted when `params` is empty).
pub fn to_param_json(title: &str, params: &[Parameter]) -> String {
    let mut out = String::from("{");
    if !params.is_empty() {
        out.push_str("\"parameters\":[");
        for (i, param) in params.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            write_parameter_object(&mut out, param);
        }
        out.push_str("],");
    }
    out.push_str("\"title\":");
    push_json_string(&mut out, title);
    out.push('}');
    out
}

/// Formats an `f64` matching OpenSCAD's `.param` JSON serializer
/// (`nlohmann::json`): shortest round-trip digits, fixed-point with trailing
/// `.0` for integer-valued numbers when `-4 <= exponent <= 14`, and scientific
/// notation with explicit sign and >= 2 exponent digits otherwise.
pub fn format_json_number(n: f64) -> String {
    if !n.is_finite() {
        return "null".to_string();
    }
    let sign = if n.is_sign_negative() { "-" } else { "" };
    if n == 0.0 {
        return format!("{sign}0.0");
    }

    let sci = format!("{:e}", n.abs());
    let (mantissa, exp_str) = sci.split_once('e').expect("scientific format");
    let exp: i32 = exp_str.parse().expect("valid exponent");
    let digits: String = mantissa.chars().filter(|&c| c != '.').collect();

    if (-4..=14).contains(&exp) {
        if exp < 0 {
            let zeros = "0".repeat((-exp - 1) as usize);
            format!("{sign}0.{zeros}{digits}")
        } else {
            let int_digits = (exp as usize) + 1;
            if digits.len() <= int_digits {
                let zeros = "0".repeat(int_digits - digits.len());
                format!("{sign}{digits}{zeros}.0")
            } else {
                format!("{sign}{}.{}", &digits[..int_digits], &digits[int_digits..])
            }
        }
    } else {
        let mant = if digits.len() == 1 {
            digits
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        let exp_sign = if exp < 0 { '-' } else { '+' };
        format!("{sign}{mant}e{exp_sign}{:02}", exp.abs())
    }
}

fn write_parameter_object(out: &mut String, param: &Parameter) {
    // Keys must appear in strict alphabetical order:
    // caption, group, initial, max, maxLength, min, name, options, step, type
    out.push('{');
    let mut first = true;
    let mut field = |out: &mut String, key: &str| {
        if !first {
            out.push(',');
        }
        first = false;
        push_json_string(out, key);
        out.push(':');
    };

    if let Some(caption) = &param.caption {
        field(out, "caption");
        push_json_string(out, caption);
    }

    if let Some(group) = &param.group {
        field(out, "group");
        push_json_string(out, group);
    }

    field(out, "initial");
    write_param_value(out, &param.default);

    match &param.constraint {
        ParamConstraint::NumberRange { max, .. } => {
            field(out, "max");
            out.push_str(&format_json_number(*max));
        }
        ParamConstraint::Vector { range: Some(range) } => {
            field(out, "max");
            out.push_str(&format_json_number(range.max));
        }
        _ => {}
    }

    if let ParamConstraint::StringText {
        max_length: Some(max_len),
    } = &param.constraint
    {
        field(out, "maxLength");
        out.push_str(&max_len.to_string());
    }

    match &param.constraint {
        ParamConstraint::NumberRange { min, .. } => {
            field(out, "min");
            out.push_str(&format_json_number(*min));
        }
        ParamConstraint::Vector { range: Some(range) } => {
            field(out, "min");
            out.push_str(&format_json_number(range.min));
        }
        _ => {}
    }

    field(out, "name");
    push_json_string(out, &param.name);

    match &param.constraint {
        ParamConstraint::NumberEnum { options } => {
            field(out, "options");
            write_number_options(out, options);
        }
        ParamConstraint::StringEnum { options } => {
            field(out, "options");
            write_string_options(out, options);
        }
        _ => {}
    }

    match &param.constraint {
        ParamConstraint::NumberSpinbox { step } | ParamConstraint::NumberRange { step, .. } => {
            field(out, "step");
            out.push_str(&format_json_number(*step));
        }
        ParamConstraint::Vector { range: Some(range) } => {
            field(out, "step");
            out.push_str(&format_json_number(range.step));
        }
        _ => {}
    }

    field(out, "type");
    let ty = match &param.default {
        ParamValue::Number(_) | ParamValue::Vector(_) => "number",
        ParamValue::Bool(_) => "boolean",
        ParamValue::String(_) => "string",
    };
    push_json_string(out, ty);

    out.push('}');
}

fn write_param_value(out: &mut String, value: &ParamValue) {
    match value {
        ParamValue::Number(n) => out.push_str(&format_json_number(*n)),
        ParamValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        ParamValue::String(s) => push_json_string(out, s),
        ParamValue::Vector(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&format_json_number(*item));
            }
            out.push(']');
        }
    }
}

fn write_number_options(out: &mut String, options: &[NumberOption]) {
    out.push('[');
    for (i, opt) in options.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        push_json_string(out, &opt.name);
        out.push_str(",\"value\":");
        out.push_str(&format_json_number(opt.value));
        out.push('}');
    }
    out.push(']');
}

fn write_string_options(out: &mut String, options: &[StringOption]) {
    out.push('[');
    for (i, opt) in options.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        push_json_string(out, &opt.name);
        out.push_str(",\"value\":");
        push_json_string(out, &opt.value);
        out.push('}');
    }
    out.push(']');
}

fn push_json_string(out: &mut String, s: &str) {
    out.push_str(&serde_json::to_string(s).expect("string serialization cannot fail"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_float_formatting_matches_openscad() {
        let cases = [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (-0.25, "-0.25"),
            (0.3, "0.3"),
            (0.0001, "0.0001"),
            (0.00009, "9e-05"),
            (0.000015, "1.5e-05"),
            (1e14, "100000000000000.0"),
            (1.5e14, "150000000000000.0"),
            (1.234567890123456e14, "123456789012345.6"),
            (1e15, "1e+15"),
            (1.5e15, "1.5e+15"),
            (1e20, "1e+20"),
        ];
        for (input, expected) in cases {
            assert_eq!(format_json_number(input), expected, "input={input:e}");
        }
    }
}
