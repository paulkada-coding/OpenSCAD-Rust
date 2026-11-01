//! OpenSCAD Customizer support: parameter extraction from `.scad` comment
//! annotations, `.param` JSON export, value-to-literal formatting, and
//! `<stem>.json` preset management.
//!
//! This crate sits in the engine layer (`scad-syntax` + `geometry`) and has no
//! UI dependencies, so both `scadr` and the desktop `app` share identical
//! parameter and preset semantics.

mod extract;
mod param_json;
mod presets;

use geometry::format_number_csg;
use serde::{Deserialize, Serialize};

pub use extract::{extract_parameters, extract_parameters_with_defines};
pub use param_json::{format_json_number, to_param_json};
pub use presets::{
    PresetError, PresetFile, coerce_preset_value, preset_path_for_scad, read_preset_file,
    resolve_preset_overrides, write_file_atomic, write_preset_file,
};

/// Default group name assigned by OpenSCAD before any `/* [Group] */` header.
pub const DEFAULT_GROUP: &str = "Parameters";

/// Special case-sensitive group name that hides subsequent parameters.
pub const HIDDEN_GROUP: &str = "Hidden";

/// Result of extracting Customizer parameters from a `.scad` source file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtractedParams {
    pub parameters: Vec<Parameter>,
    pub warnings: Vec<ParamWarning>,
}

/// A single extracted Customizer parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    /// Group/tab name (`Some("Parameters")` by default, `None` after an
    /// unbracketed single-line block comment `/* ... */`).
    pub group: Option<String>,
    /// Optional description from a `// ...` comment on the immediately
    /// preceding line.
    pub caption: Option<String>,
    pub default: ParamValue,
    pub constraint: ParamConstraint,
    /// 1-based line number of the assignment in the main `.scad` file.
    pub line: usize,
}

/// Typed value of a Customizer parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParamValue {
    Number(f64),
    Bool(bool),
    String(String),
    Vector(Vec<f64>),
}

impl ParamValue {
    /// Returns `true` when `self` and `other` have the same parameter kind
    /// (and, for vectors, the same length).
    pub fn same_kind(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number(_), Self::Number(_))
            | (Self::Bool(_), Self::Bool(_))
            | (Self::String(_), Self::String(_)) => true,
            (Self::Vector(a), Self::Vector(b)) => a.len() == b.len(),
            _ => false,
        }
    }

    /// Compares two parameter values for semantic equality (`-0.0 == 0.0`).
    pub fn values_equal(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Vector(a), Self::Vector(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x == y)
            }
            _ => false,
        }
    }

    /// Formats the value as an OpenSCAD literal suitable for
    /// `EvalOptions::defines` and [`scad_syntax::replace_assignment_value`].
    pub fn to_scad_literal(&self) -> String {
        match self {
            Self::Number(n) => format_scad_number(*n),
            Self::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            Self::String(s) => escape_scad_string(s),
            Self::Vector(items) => {
                let mut out = String::from("[");
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&format_scad_number(*item));
                }
                out.push(']');
                out
            }
        }
    }

    /// Formats the value as stored inside OpenSCAD's `<stem>.json`
    /// `parameterSets` map (where strings are stored unquoted).
    pub fn to_preset_string(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            _ => self.to_scad_literal(),
        }
    }
}

/// Widget / validation constraint parsed from a trailing comment.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamConstraint {
    /// Boolean checkbox (`true` / `false`).
    UnconstrainedBool,
    /// Scalar numeric spinbox (`step` defaults to `1.0`).
    NumberSpinbox { step: f64 },
    /// Scalar numeric slider + spinbox (`[max]`, `[min:max]`, `[min:step:max]`).
    NumberRange { min: f64, max: f64, step: f64 },
    /// Dropdown of labeled numeric choices (`[10, 20]` or `[10:Small, 20:Large]`).
    NumberEnum { options: Vec<NumberOption> },
    /// Single-line text box with optional character limit (`// 8`).
    StringText { max_length: Option<usize> },
    /// Dropdown of labeled string choices (`[a, b]` or `[a:Alpha, b:Beta]`).
    StringEnum { options: Vec<StringOption> },
    /// Numeric vector of 1..=4 components with optional per-component range.
    Vector { range: Option<VectorRange> },
}

/// Labeled numeric choice in a dropdown constraint.
#[derive(Debug, Clone, PartialEq)]
pub struct NumberOption {
    pub name: String,
    pub value: f64,
}

/// Labeled string choice in a dropdown constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringOption {
    pub name: String,
    pub value: String,
}

/// Component range constraint on a numeric vector parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VectorRange {
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

/// Warning emitted when a parameter's initial value lies outside its declared
/// `[min:max]` range and OpenSCAD expands the bound to include it.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamWarning {
    pub name: String,
    pub value: f64,
    pub declared_bound: f64,
    pub kind: ParamWarningKind,
    /// 1-based line number in the `.scad` file.
    pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamWarningKind {
    BelowMinimum,
    AboveMaximum,
}

impl ParamWarning {
    /// Formats the warning exactly as OpenSCAD prints it to stderr.
    pub fn format_openscad(&self, file_name: &str) -> String {
        let val = format_number_csg(self.value);
        let bound = format_number_csg(self.declared_bound);
        match self.kind {
            ParamWarningKind::BelowMinimum => format!(
                "WARNING: Parameter \"{}\": value {val} is below the declared minimum {bound}, adjusting minimum value in file {file_name}, line {}",
                self.name, self.line
            ),
            ParamWarningKind::AboveMaximum => format!(
                "WARNING: Parameter \"{}\": value {val} is above the declared maximum {bound}, adjusting maximum value in file {file_name}, line {}",
                self.name, self.line
            ),
        }
    }
}

fn format_scad_number(n: f64) -> String {
    if !n.is_finite() {
        return "0".to_string();
    }
    if n == 0.0 {
        return "0".to_string();
    }
    let abs = n.abs();
    // Avoid emitting >= 16-digit bare integer tokens (which trigger OpenSCAD's
    // ImpreciseInteger lexer warning) or long leading-zero decimals.
    if !(1e-6..1e15).contains(&abs) {
        return format!("{n:e}");
    }
    n.to_string()
}

fn escape_scad_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use scad_syntax::{parse_number_literal, unescape_string_literal};

    #[test]
    fn scad_literals_round_trip_losslessly() {
        for n in [
            0.0,
            -0.0,
            1.0,
            -0.25,
            0.3,
            0.1 + 0.2,
            70.0,
            1e-9,
            1.5e20,
            -999_999.125,
        ] {
            let lit = ParamValue::Number(n).to_scad_literal();
            let parsed = if let Some(rest) = lit.strip_prefix('-') {
                -parse_number_literal(rest)
            } else {
                parse_number_literal(&lit)
            };
            assert_eq!(parsed, if n == 0.0 { 0.0 } else { n }, "literal={lit}");
        }

        for s in ["stampa", "", "a \"b\" \\ c\nd\te", "caffè €"] {
            let lit = ParamValue::String(s.to_string()).to_scad_literal();
            assert_eq!(unescape_string_literal(&lit), s);
        }

        assert_eq!(
            ParamValue::Vector(vec![0.2, -0.25, 3.0]).to_scad_literal(),
            "[0.2, -0.25, 3]"
        );
        assert_eq!(ParamValue::Bool(true).to_scad_literal(), "true");
        assert_eq!(ParamValue::Bool(false).to_scad_literal(), "false");
    }
}
