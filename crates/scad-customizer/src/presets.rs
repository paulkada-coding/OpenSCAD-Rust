//! OpenSCAD-compatible `<stem>.json` preset reading, validation, and writing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use thiserror::Error;

use crate::{ParamConstraint, ParamValue, Parameter};

fn default_format_version() -> String {
    "1".to_string()
}

/// Errors raised while reading or writing a Customizer preset JSON file.
#[derive(Debug, Error)]
pub enum PresetError {
    #[error("cannot open preset file '{}': {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid preset JSON in '{}': {source}", path.display())]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

impl PresetError {
    /// Formats the error as OpenSCAD prints it on stderr when `-p` fails.
    pub fn format_openscad(&self) -> String {
        match self {
            Self::Io { path, .. } => format!(
                "ERROR: Cannot open Parameter Set '{}' for reading",
                path.display()
            ),
            Self::Json { path, source } => {
                format!(
                    "ERROR: Cannot open Parameter Set '{}': {source}",
                    path.display()
                )
            }
        }
    }
}

/// In-memory representation of an OpenSCAD `<stem>.json` preset file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetFile {
    #[serde(rename = "fileFormatVersion", default = "default_format_version")]
    pub file_format_version: String,
    #[serde(rename = "parameterSets", default)]
    pub parameter_sets: BTreeMap<String, BTreeMap<String, JsonValue>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, JsonValue>,
}

impl Default for PresetFile {
    fn default() -> Self {
        Self {
            file_format_version: default_format_version(),
            parameter_sets: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }
}

impl PresetFile {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_names(&self) -> impl Iterator<Item = &str> {
        self.parameter_sets.keys().map(String::as_str)
    }

    pub fn contains_set(&self, set_name: &str) -> bool {
        self.parameter_sets.contains_key(set_name)
    }

    /// Inserts or replaces `set_name` with the given parameter values,
    /// formatted using OpenSCAD's string-encoded convention.
    pub fn upsert_set(&mut self, set_name: impl Into<String>, values: &[(String, ParamValue)]) {
        if self.file_format_version.is_empty() {
            self.file_format_version = default_format_version();
        }
        let mut map = BTreeMap::new();
        for (name, val) in values {
            map.insert(name.clone(), JsonValue::String(val.to_preset_string()));
        }
        self.parameter_sets.insert(set_name.into(), map);
    }

    /// Renames `old_name` to `new_name`. Returns `false` if `old_name` did not
    /// exist.
    pub fn rename_set(&mut self, old_name: &str, new_name: impl Into<String>) -> bool {
        let new_name = new_name.into();
        if old_name == new_name {
            return self.parameter_sets.contains_key(old_name);
        }
        let Some(entries) = self.parameter_sets.remove(old_name) else {
            return false;
        };
        self.parameter_sets.insert(new_name, entries);
        true
    }

    /// Deletes `set_name`, returning `true` if it existed.
    pub fn delete_set(&mut self, set_name: &str) -> bool {
        self.parameter_sets.remove(set_name).is_some()
    }

    /// Serializes the preset file as 4-space indented JSON with a trailing
    /// newline, matching OpenSCAD's desktop Customizer output.
    pub fn to_pretty_json(&self) -> Result<String, serde_json::Error> {
        let mut buf = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
        self.serialize(&mut ser)?;
        buf.push(b'\n');
        Ok(String::from_utf8(buf).expect("serde_json emits valid UTF-8"))
    }
}

/// Companion preset path for `scad_path` (`foo/model.scad` → `foo/model.json`).
pub fn preset_path_for_scad(scad_path: &Path) -> PathBuf {
    scad_path.with_extension("json")
}

/// Reads and parses a Customizer preset JSON file.
pub fn read_preset_file(path: &Path) -> Result<PresetFile, PresetError> {
    let text = std::fs::read_to_string(path).map_err(|source| PresetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|source| PresetError::Json {
        path: path.to_path_buf(),
        source,
    })
}

/// Writes `presets` to `path` as 4-space pretty JSON.
pub fn write_preset_file(path: &Path, presets: &PresetFile) -> Result<(), PresetError> {
    let json = presets
        .to_pretty_json()
        .map_err(|source| PresetError::Json {
            path: path.to_path_buf(),
            source,
        })?;
    std::fs::write(path, json).map_err(|source| PresetError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Resolves and validates all overrides from `set_name` against `params`.
///
/// Unknown parameter names, hidden variables, and invalid/mismatched values are
/// ignored, while out-of-range numbers/vectors are clamped and overlong strings
/// are truncated to `maxLength` — matching `openscad -p <file> -P <set>`.
pub fn resolve_preset_overrides(
    presets: &PresetFile,
    set_name: &str,
    params: &[Parameter],
) -> Vec<(String, ParamValue)> {
    let Some(entries) = presets.parameter_sets.get(set_name) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for param in params {
        if let Some(raw) = entries.get(&param.name)
            && let Some(val) = coerce_preset_value(param, raw)
        {
            out.push((param.name.clone(), val));
        }
    }
    out
}

/// Coerces and validates a raw JSON value from `parameterSets` against `param`'s
/// type and constraint.
pub fn coerce_preset_value(param: &Parameter, raw: &JsonValue) -> Option<ParamValue> {
    match &param.default {
        ParamValue::Number(_) => {
            let v = parse_json_number(raw)?;
            match &param.constraint {
                ParamConstraint::NumberEnum { options } => options
                    .iter()
                    .find(|o| o.value == v)
                    .map(|o| ParamValue::Number(o.value)),
                ParamConstraint::NumberRange { min, max, .. } => {
                    let lo = min.min(*max);
                    let hi = min.max(*max);
                    Some(ParamValue::Number(v.clamp(lo, hi)))
                }
                _ => Some(ParamValue::Number(v)),
            }
        }
        ParamValue::Bool(_) => match raw {
            JsonValue::Bool(b) => Some(ParamValue::Bool(*b)),
            JsonValue::String(s) => match s.trim() {
                "true" => Some(ParamValue::Bool(true)),
                "false" => Some(ParamValue::Bool(false)),
                _ => None,
            },
            _ => None,
        },
        ParamValue::String(_) => {
            let JsonValue::String(s) = raw else {
                return None;
            };
            match &param.constraint {
                ParamConstraint::StringEnum { options } => options
                    .iter()
                    .find(|o| o.value == *s)
                    .map(|o| ParamValue::String(o.value.clone())),
                ParamConstraint::StringText {
                    max_length: Some(max_len),
                } => Some(ParamValue::String(s.chars().take(*max_len).collect())),
                _ => Some(ParamValue::String(s.clone())),
            }
        }
        ParamValue::Vector(default_elems) => {
            let mut elems = parse_json_vector(raw)?;
            if elems.len() != default_elems.len() {
                return None;
            }
            if let ParamConstraint::Vector { range: Some(r) } = &param.constraint {
                let lo = r.min.min(r.max);
                let hi = r.min.max(r.max);
                for x in &mut elems {
                    *x = x.clamp(lo, hi);
                }
            }
            Some(ParamValue::Vector(elems))
        }
    }
}

fn parse_json_number(raw: &JsonValue) -> Option<f64> {
    match raw {
        JsonValue::Number(n) => n.as_f64().filter(|v| v.is_finite()),
        JsonValue::String(s) => s.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
        _ => None,
    }
}

fn parse_json_vector(raw: &JsonValue) -> Option<Vec<f64>> {
    match raw {
        JsonValue::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                out.push(parse_json_number(item)?);
            }
            Some(out)
        }
        JsonValue::String(s) => {
            let inner = s.trim().strip_prefix('[')?.strip_suffix(']')?.trim();
            if inner.is_empty() {
                return Some(Vec::new());
            }
            let mut out = Vec::new();
            for part in inner.split(',') {
                let n = part.trim().parse::<f64>().ok().filter(|v| v.is_finite())?;
                out.push(n);
            }
            Some(out)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract_parameters;

    #[test]
    fn preset_coercion_clamps_truncates_and_filters_like_openscad() {
        let scad = r#"
num_range = 10; // [0:0.5:20]
num_enum  = 10; // [10, 20, 30]
flag      = true;
label     = "ROSSO"; // 4
mode      = "a"; // [a, b]
vec3      = [1, 2, 3]; // [-5:5]
/* [Hidden] */
secret    = 99;
"#;
        let extracted = extract_parameters(scad);
        let json = r#"{
            "fileFormatVersion": "1",
            "extraTopKey": 123,
            "parameterSets": {
                " clamped ": {
                    "num_range": "999",
                    "num_enum": "40",
                    "flag": "false",
                    "label": "ABCDEFGH",
                    "mode": "b",
                    "vec3": "[-10, 2.5, 50]",
                    "secret": "1",
                    "unknown": "7"
                },
                "wrong_vec": {
                    "vec3": "[1, 2]"
                }
            }
        }"#;
        let mut presets: PresetFile = serde_json::from_str(json).unwrap();
        let overrides = resolve_preset_overrides(&presets, " clamped ", &extracted.parameters);
        assert_eq!(
            overrides,
            vec![
                ("num_range".into(), ParamValue::Number(20.0)),
                ("flag".into(), ParamValue::Bool(false)),
                ("label".into(), ParamValue::String("ABCD".into())),
                ("mode".into(), ParamValue::String("b".into())),
                ("vec3".into(), ParamValue::Vector(vec![-5.0, 2.5, 5.0])),
            ]
        );

        assert!(resolve_preset_overrides(&presets, "wrong_vec", &extracted.parameters).is_empty());

        presets.upsert_set("custom", &[("mode".into(), ParamValue::String("a".into()))]);
        assert!(presets.rename_set("custom", "renamed"));
        assert!(presets.contains_set("renamed"));
        assert!(!presets.contains_set("custom"));
        assert!(presets.delete_set("wrong_vec"));

        let serialized = presets.to_pretty_json().unwrap();
        assert!(serialized.contains("\"fileFormatVersion\": \"1\""));
        assert!(serialized.contains("\"extraTopKey\": 123"));
        assert!(serialized.contains("\"renamed\""));
    }
}
