//! Builtin `fontmetrics()` and `textmetrics()` functions backed by [`scad_font`].

use scad_font::{TextParams, measure_font, measure_text};
use scad_syntax::ast::Span;

use crate::eval::Evaluator;
use crate::value::{Arguments, Value};

/// Bind positional and named arguments following OpenSCAD parameter order.
struct BoundArgs<'a> {
    bound: Vec<(&'static str, &'a Value)>,
}

impl<'a> BoundArgs<'a> {
    fn new(args: &'a Arguments, names: &'static [&'static str]) -> Self {
        let mut bound: Vec<(&'static str, &'a Value)> = Vec::new();
        for (name, val) in &args.named {
            if let Some(&canon) = names.iter().find(|&&p| p == name.as_str()) {
                bound.push((canon, val));
            }
        }
        let mut pos = args.positional.iter();
        for &param in names {
            if bound.iter().any(|(n, _)| *n == param) {
                continue;
            }
            if let Some(val) = pos.next() {
                bound.push((param, val));
            }
        }
        Self { bound }
    }

    fn string(
        &self,
        ev: &mut Evaluator,
        fn_name: &str,
        param: &str,
        default: &str,
        span: Span,
    ) -> String {
        match self
            .bound
            .iter()
            .find(|(n, _)| *n == param)
            .map(|(_, v)| *v)
        {
            None | Some(Value::Undef) => default.to_string(),
            Some(Value::String(s)) => s.to_string(),
            Some(other) => {
                ev.warning(
                    format!(
                        "{fn_name}(..., {param}={}) Invalid type: expected string, found {}",
                        other.to_repr(),
                        other.type_name()
                    ),
                    span,
                );
                default.to_string()
            }
        }
    }

    fn number(
        &self,
        ev: &mut Evaluator,
        fn_name: &str,
        param: &str,
        default: f64,
        span: Span,
    ) -> f64 {
        match self
            .bound
            .iter()
            .find(|(n, _)| *n == param)
            .map(|(_, v)| *v)
        {
            None | Some(Value::Undef) => default,
            Some(Value::Number(n)) if n.is_finite() => *n,
            Some(Value::Number(_)) => default,
            Some(other) => {
                ev.warning(
                    format!(
                        "{fn_name}(..., {param}={}) Invalid type: expected number, found {}",
                        other.to_repr(),
                        other.type_name()
                    ),
                    span,
                );
                default
            }
        }
    }
}

/// Evaluates `fontmetrics([size[, font]])`.
pub fn fontmetrics(ev: &mut Evaluator, args: &Arguments, span: Span) -> Value {
    let bound = BoundArgs::new(args, &["size", "font"]);
    let size = bound.number(ev, "fontmetrics", "size", 10.0, span);
    let font_spec = bound.string(ev, "fontmetrics", "font", "", span);

    let Some(m) = measure_font(size, &font_spec) else {
        return Value::Undef;
    };

    Value::object(vec![
        (
            "nominal".into(),
            Value::object(vec![
                ("ascent".into(), Value::Number(m.nominal_ascent)),
                ("descent".into(), Value::Number(m.nominal_descent)),
            ]),
        ),
        (
            "max".into(),
            Value::object(vec![
                ("ascent".into(), Value::Number(m.max_ascent)),
                ("descent".into(), Value::Number(m.max_descent)),
            ]),
        ),
        ("interline".into(), Value::Number(m.interline)),
        (
            "font".into(),
            Value::object(vec![
                ("family".into(), Value::from(m.family.as_str())),
                ("style".into(), Value::from(m.style.as_str())),
            ]),
        ),
    ])
}

/// Evaluates `textmetrics(text[, size, font, halign, valign, spacing, direction, language, script])`.
pub fn textmetrics(ev: &mut Evaluator, args: &Arguments, span: Span) -> Value {
    let bound = BoundArgs::new(
        args,
        &[
            "text",
            "size",
            "font",
            "halign",
            "valign",
            "spacing",
            "direction",
            "language",
            "script",
        ],
    );
    let text = bound.string(ev, "textmetrics", "text", "", span);
    let size = bound.number(ev, "textmetrics", "size", 10.0, span);
    let font = bound.string(ev, "textmetrics", "font", "", span);
    let halign = bound.string(ev, "textmetrics", "halign", "default", span);
    let valign = bound.string(ev, "textmetrics", "valign", "default", span);
    let spacing = bound.number(ev, "textmetrics", "spacing", 1.0, span);
    let direction = bound.string(ev, "textmetrics", "direction", "", span);
    let language = bound.string(ev, "textmetrics", "language", "en", span);
    let script = bound.string(ev, "textmetrics", "script", "", span);

    let m = measure_text(&TextParams {
        text: &text,
        size,
        font: &font,
        halign: &halign,
        valign: &valign,
        spacing,
        direction: &direction,
        language: &language,
        script: &script,
    });

    Value::object(vec![
        (
            "position".into(),
            Value::list(vec![m.position[0].into(), m.position[1].into()]),
        ),
        (
            "size".into(),
            Value::list(vec![m.size[0].into(), m.size[1].into()]),
        ),
        ("ascent".into(), Value::Number(m.ascent)),
        ("descent".into(), Value::Number(m.descent)),
        (
            "offset".into(),
            Value::list(vec![m.offset[0].into(), m.offset[1].into()]),
        ),
        (
            "advance".into(),
            Value::list(vec![m.advance[0].into(), m.advance[1].into()]),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{EvalOptions, Evaluator};

    fn run_logs(src: &str) -> Vec<String> {
        let mut ev = Evaluator::new(EvalOptions::default());
        let (_, err) = ev.run(Path::new("test.scad"), src);
        assert!(err.is_none());
        ev.messages().iter().map(|m| m.text.clone()).collect()
    }

    #[test]
    fn matches_openscad_textmetrics_and_fontmetrics() {
        let logs = run_logs(
            r#"
            echo(textmetrics("a"));
            echo(textmetrics(""));
            echo(fontmetrics());
            echo(textmetrics("ab", size=10));
            echo(textmetrics("Hi", size = 20, font = "Liberation Sans:style=Bold", halign = "center", valign = "center", spacing = 1.5, direction = "ttb"));
            echo(fontmetrics(size = 20, font = "Liberation Sans:style=Bold"));
            echo(textmetrics("gp", size = 10, halign = "center", valign = "center"));
            echo(textmetrics("   ", size = 10));
            "#,
        );
        assert_eq!(
            logs[0],
            "{ position = [0.5888, -0.1408]; size = [7.136, 7.616]; ascent = 7.4752; descent = -0.1408; offset = [0, 0]; advance = [7.7243, 0]; }"
        );
        assert_eq!(
            logs[1],
            "{ position = [0, 0]; size = [0, 0]; ascent = 0; descent = 0; offset = [0, 0]; advance = [0, 0]; }"
        );
        assert_eq!(
            logs[2],
            r#"{ nominal = { ascent = 12.5733; descent = -2.9433; }; max = { ascent = 13.6109; descent = -4.2114; }; interline = 15.9709; font = { family = "Liberation Sans"; style = "Regular"; }; }"#
        );
        assert_eq!(
            logs[3],
            "{ position = [0.5888, -0.1408]; size = [14.2779, 10.208]; ascent = 10.0672; descent = -0.1408; offset = [0, 0]; advance = [15.4486, 0]; }"
        );
        assert_eq!(
            logs[4],
            "{ position = [-8.174, -23.0984]; size = [16.3328, 58.729]; ascent = 20.1344; descent = 0; offset = [0, 39.1032]; advance = [0, -78.2064]; }"
        );
        assert_eq!(
            logs[5],
            r#"{ nominal = { ascent = 25.1466; descent = -5.8866; }; max = { ascent = 28.7002; descent = -10.4574; }; interline = 31.9418; font = { family = "Liberation Sans"; style = "Bold"; }; }"#
        );
        assert_eq!(
            logs[6],
            "{ position = [-7.1419, -5.1776]; size = [14.2843, 10.3552]; ascent = 7.4688; descent = -2.8864; offset = [-7.7243, -2.2912]; advance = [15.4486, 0]; }"
        );
        assert_eq!(
            logs[7],
            "{ position = [0, 0]; size = [0, 0]; ascent = 0; descent = 0; offset = [0, 0]; advance = [11.5764, 0]; }"
        );
    }
}
