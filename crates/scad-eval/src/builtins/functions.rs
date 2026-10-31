//! Builtin functions.

use std::cmp::Ordering;

use scad_syntax::ast::Span;

use crate::eval::Evaluator;
use crate::message::EvalResult;
use crate::value::{Arguments, Value};

const NAMES: &[&str] = &[
    "abs", "sign", "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "floor", "ceil",
    "round", "ln", "log", "exp", "pow", "sqrt", "min", "max", "norm", "cross", "len", "concat",
    "lookup", "str", "chr", "ord", "search", "rands", "version", "version_num", "is_undef",
    "is_bool", "is_num", "is_string", "is_list", "is_function", "is_object", "parent_module",
];

pub fn exists(name: &str) -> bool {
    NAMES.contains(&name)
}

/// OpenSCAD's version reported by `version()`: the nightly we target.
const VERSION: [f64; 3] = [2026.0, 10.0, 22.0];

/// Expected argument count for builtins that check it.
fn arity(name: &str) -> Option<usize> {
    match name {
        "abs" | "sign" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "floor" | "ceil"
        | "round" | "ln" | "exp" | "sqrt" | "norm" => Some(1),
        "atan2" | "pow" | "lookup" | "cross" | "log" => Some(2),
        _ => None,
    }
}

/// `"argument 0: expected number, found string ("a")"`
fn conversion_warning(ev: &mut Evaluator, name: &str, index: usize, expected: &str, found: &Value, span: Span) {
    ev.warning(
        format!(
            "{name}() parameter could not be converted: argument {index}: expected {expected}, found {} ({})",
            found.type_name(),
            found.to_repr()
        ),
        span,
    );
}

pub fn call(ev: &mut Evaluator, name: &str, args: Arguments, span: Span) -> EvalResult<Value> {
    let a = &args.positional;
    if let Some(expected) = arity(name) {
        let accepted = a.len() == expected || (name == "log" && a.len() == 1);
        if !accepted {
            ev.warning(
                format!(
                    "{name}() number of parameters does not match: expected {expected}, found {}",
                    a.len()
                ),
                span,
            );
            return Ok(Value::Undef);
        }
    }
    let num = |i: usize| a.get(i).and_then(Value::as_number);
    let unary = |f: fn(f64) -> f64| num(0).map_or(Value::Undef, |x| Value::Number(f(x)));

    Ok(match name {
        "abs" => unary(f64::abs),
        "sign" => unary(|x| {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                0.0
            }
        }),
        "sin" => unary(sin_degrees),
        "cos" => unary(cos_degrees),
        "tan" => unary(tan_degrees),
        "asin" => unary(asin_degrees),
        "acos" => unary(acos_degrees),
        "atan" => unary(|x| x.atan().to_degrees()),
        "atan2" => match (num(0), num(1)) {
            (Some(y), Some(x)) => Value::Number(y.atan2(x).to_degrees()),
            _ => Value::Undef,
        },
        "floor" => unary(f64::floor),
        "ceil" => unary(f64::ceil),
        "round" => unary(round_half_away),
        "ln" => unary(f64::ln),
        "log" => match (num(0), num(1)) {
            (Some(base), Some(x)) => Value::Number(x.ln() / base.ln()),
            (Some(x), None) => Value::Number(x.log10()),
            _ => Value::Undef,
        },
        "exp" => unary(f64::exp),
        "sqrt" => unary(f64::sqrt),
        "pow" => match (num(0), num(1)) {
            (Some(b), Some(e)) => Value::Number(b.powf(e)),
            _ => Value::Undef,
        },
        "min" | "max" => min_max(ev, name, a, span),
        "norm" => match &a[0] {
            Value::List(items) => match items
                .iter()
                .map(|v| v.as_number().map(|n| n * n))
                .sum::<Option<f64>>()
            {
                Some(sum) => Value::Number(sum.sqrt()),
                None => {
                    ev.warning("Incorrect arguments to norm()", span);
                    Value::Undef
                }
            },
            other => {
                conversion_warning(ev, name, 0, "vector", other, span);
                Value::Undef
            }
        },
        "cross" => {
            let result = cross(a);
            if result.is_undef() {
                ev.warning("Invalid vector size of parameter for cross()", span);
            }
            result
        }
        "len" => match a.first() {
            Some(Value::List(items)) => Value::Number(items.len() as f64),
            Some(Value::String(s)) => Value::Number(s.chars().count() as f64),
            other => {
                conversion_warning(ev, name, 0, "string", other.unwrap_or(&Value::Undef), span);
                Value::Undef
            }
        },
        "concat" => {
            let mut out = Vec::new();
            for value in a {
                match value {
                    Value::List(items) => out.extend(items.iter().cloned()),
                    other => out.push(other.clone()),
                }
            }
            Value::list(out)
        }
        "lookup" => lookup(a),
        "str" => Value::string(a.iter().map(Value::to_str_string).collect::<String>()),
        "chr" => {
            let mut out = String::new();
            for value in a {
                push_chars(value, &mut out);
            }
            Value::string(out)
        }
        // The first character's code point.
        "ord" => match a.first() {
            Some(Value::String(s)) => s
                .chars()
                .next()
                .map_or(Value::Undef, |c| Value::Number(f64::from(u32::from(c)))),
            other => {
                conversion_warning(ev, name, 0, "string", other.unwrap_or(&Value::Undef), span);
                Value::Undef
            }
        },
        "search" => search(a),
        "rands" => rands(ev, a),
        "version" => Value::list(VERSION.iter().map(|&n| Value::Number(n)).collect()),
        "version_num" => Value::Number(VERSION[0] * 10000.0 + VERSION[1] * 100.0 + VERSION[2]),
        "is_undef" => Value::Bool(a.first().is_none_or(Value::is_undef)),
        "is_bool" => Value::Bool(matches!(a.first(), Some(Value::Bool(_)))),
        "is_num" => Value::Bool(matches!(a.first(), Some(Value::Number(n)) if !n.is_nan())),
        "is_string" => Value::Bool(matches!(a.first(), Some(Value::String(_)))),
        "is_list" => Value::Bool(matches!(a.first(), Some(Value::List(_)))),
        "is_function" => Value::Bool(matches!(a.first(), Some(Value::Function(_)))),
        // Objects are an experimental feature we do not support yet.
        "is_object" => Value::Bool(false),
        "parent_module" => {
            let n = num(0).unwrap_or(1.0).max(0.0) as usize;
            let count = ev.module_names.len();
            if n >= count {
                ev.warning(
                    format!(
                        "Parent module index ({n}) greater than the number of modules on the stack"
                    ),
                    span,
                );
                Value::Undef
            } else {
                Value::string(ev.module_names[count - 1 - n].as_str())
            }
        }
        _ => {
            ev.warning(format!("Ignoring unknown function '{name}'"), span);
            Value::Undef
        }
    })
}

/// Quadrant (0..4) when `x` is an exact multiple of 90°.
fn right_angle_quadrant(x: f64) -> Option<i64> {
    let q = x / 90.0;
    (q.fract() == 0.0 && q.abs() < 1e15).then(|| (q as i64).rem_euclid(4))
}

/// Sine in degrees, exact at multiples of 30° and 45°, with OpenSCAD's
/// signs of zero (`sin(180)` is -0).
pub fn sin_degrees(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    if let Some(quadrant) = right_angle_quadrant(x) {
        return match quadrant {
            0 if x == 0.0 => x,
            0 => 0.0,
            1 => 1.0,
            2 => -0.0,
            _ => -1.0,
        };
    }
    let r = x.rem_euclid(360.0);
    let (half, root2, root3) = (0.5, 2f64.sqrt() / 2.0, 3f64.sqrt() / 2.0);
    match r {
        30.0 | 150.0 => half,
        210.0 | 330.0 => -half,
        45.0 | 135.0 => root2,
        225.0 | 315.0 => -root2,
        60.0 | 120.0 => root3,
        240.0 | 300.0 => -root3,
        _ => r.to_radians().sin(),
    }
}

/// Cosine in degrees, exact like [`sin_degrees`].
pub fn cos_degrees(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    if let Some(quadrant) = right_angle_quadrant(x) {
        return match quadrant {
            0 => 1.0,
            1 => 0.0,
            2 => -1.0,
            _ => -0.0,
        };
    }
    let r = x.rem_euclid(360.0);
    let (half, root2, root3) = (0.5, 2f64.sqrt() / 2.0, 3f64.sqrt() / 2.0);
    match r {
        60.0 | 300.0 => half,
        120.0 | 240.0 => -half,
        45.0 | 315.0 => root2,
        135.0 | 225.0 => -root2,
        30.0 | 330.0 => root3,
        150.0 | 210.0 => -root3,
        _ => r.to_radians().cos(),
    }
}

fn tan_degrees(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    if let Some(quadrant) = right_angle_quadrant(x) {
        return match quadrant {
            0 if x == 0.0 => x,
            0 => 0.0,
            1 => f64::INFINITY,
            2 => -0.0,
            _ => f64::NEG_INFINITY,
        };
    }
    let r = x.rem_euclid(180.0);
    match r {
        45.0 => 1.0,
        135.0 => -1.0,
        60.0 => 3f64.sqrt(),
        120.0 => -3f64.sqrt(),
        _ => r.to_radians().tan(),
    }
}

/// OpenSCAD rounds like `floor(x + 0.5)` (mirrored for negatives).
fn round_half_away(x: f64) -> f64 {
    if x >= 0.0 {
        (x + 0.5).floor()
    } else {
        -(-x + 0.5).floor()
    }
}

fn min_max(ev: &mut Evaluator, name: &str, args: &[Value], span: Span) -> Value {
    let arity = |ev: &mut Evaluator, what: &str| {
        ev.warning(
            format!("{name}() number of parameters does not match: expected at least 1{what}, found 0"),
            span,
        );
        Value::Undef
    };
    let (values, single_vector) = match args {
        [] => return arity(ev, ""),
        [Value::List(items)] if items.is_empty() => return arity(ev, " vector element"),
        [Value::List(items)] => (items.as_ref(), true),
        _ => (args, false),
    };
    let mut numbers = Vec::with_capacity(values.len());
    for (i, value) in values.iter().enumerate() {
        match value.as_number() {
            Some(n) => numbers.push(n),
            None => {
                // Inside a single vector argument OpenSCAD reports argument 0.
                let index = if single_vector { 0 } else { i };
                conversion_warning(ev, name, index, "number", value, span);
                return Value::Undef;
            }
        }
    }
    let wanted = if name == "max" {
        Ordering::Greater
    } else {
        Ordering::Less
    };
    Value::Number(
        numbers
            .into_iter()
            .reduce(|best, x| {
                if x.partial_cmp(&best) == Some(wanted) {
                    x
                } else {
                    best
                }
            })
            .unwrap_or(f64::NAN),
    )
}

fn asin_degrees(x: f64) -> f64 {
    match x {
        0.5 => 30.0,
        -0.5 => -30.0,
        1.0 => 90.0,
        -1.0 => -90.0,
        _ => x.asin().to_degrees(),
    }
}

fn acos_degrees(x: f64) -> f64 {
    match x {
        0.5 => 60.0,
        -0.5 => 120.0,
        0.0 => 90.0,
        1.0 => 0.0,
        -1.0 => 180.0,
        _ => x.acos().to_degrees(),
    }
}

fn cross(args: &[Value]) -> Value {
    match (args.first(), args.get(1)) {
        (Some(a), Some(b)) => {
            if let (Some([ax, ay, az]), Some([bx, by, bz])) = (a.as_vec::<3>(), b.as_vec::<3>()) {
                Value::list(vec![
                    (ay * bz - az * by).into(),
                    (az * bx - ax * bz).into(),
                    (ax * by - ay * bx).into(),
                ])
            } else if let (Some([ax, ay]), Some([bx, by])) = (a.as_vec::<2>(), b.as_vec::<2>()) {
                Value::Number(ax * by - ay * bx)
            } else {
                Value::Undef
            }
        }
        _ => Value::Undef,
    }
}

/// Piecewise-linear interpolation in a `[[key, value], ...]` table.
fn lookup(args: &[Value]) -> Value {
    let (Some(key), Some(table)) = (
        args.first().and_then(Value::as_number),
        args.get(1).and_then(Value::as_list),
    ) else {
        return Value::Undef;
    };
    let points: Vec<[f64; 2]> = table.iter().filter_map(Value::as_vec::<2>).collect();
    let Some(first) = points.first() else {
        return Value::Undef;
    };
    let mut low = *first;
    let mut high = *first;
    for p in &points {
        if p[0] <= key && (p[0] > low[0] || low[0] > key) {
            low = *p;
        }
        if p[0] >= key && (p[0] < high[0] || high[0] < key) {
            high = *p;
        }
    }
    if low[0] > key {
        // Below every key: clamp to the smallest.
        let min = points.iter().min_by(|a, b| a[0].total_cmp(&b[0])).unwrap_or(first);
        return Value::Number(min[1]);
    }
    if high[0] < key {
        let max = points.iter().max_by(|a, b| a[0].total_cmp(&b[0])).unwrap_or(first);
        return Value::Number(max[1]);
    }
    if high[0] == low[0] {
        return Value::Number(low[1]);
    }
    let t = (key - low[0]) / (high[0] - low[0]);
    Value::Number(low[1] + t * (high[1] - low[1]))
}

fn push_chars(value: &Value, out: &mut String) {
    match value {
        // Fractions are truncated; 0, negatives and invalid code points are skipped.
        Value::Number(n) => {
            if let Some(c) = (*n >= 1.0 && *n < 1_114_112.0)
                .then(|| char::from_u32(n.trunc() as u32))
                .flatten()
            {
                out.push(c);
            }
        }
        Value::List(items) => items.iter().for_each(|v| push_chars(v, out)),
        Value::Range(r) => r.iter().for_each(|n| push_chars(&Value::Number(n), out)),
        _ => {}
    }
}

/// `search(match, in, num_returns_per_match = 1, index_col_num = 0)`.
fn search(args: &[Value]) -> Value {
    let (Some(needle), Some(haystack)) = (args.first(), args.get(1)) else {
        return Value::Undef;
    };
    let limit = args.get(2).and_then(Value::as_number).unwrap_or(1.0).max(0.0) as usize;
    let column = args.get(3).and_then(Value::as_number).unwrap_or(0.0).max(0.0) as usize;

    let candidates: Vec<Value> = match haystack {
        Value::List(items) => items.to_vec(),
        Value::String(s) => s.chars().map(|c| Value::string(c.to_string())).collect(),
        _ => return Value::Undef,
    };
    // A list target is compared with whole rows, anything else with the
    // row's `column` entry.
    let matches = |item: &Value, target: &Value| -> bool {
        match (item, target) {
            (Value::List(_), Value::List(_)) => item.equals(target),
            (Value::List(row), _) => row.get(column).is_some_and(|v| v.equals(target)),
            _ => item.equals(target),
        }
    };
    let find = |target: &Value| -> Vec<Value> {
        let mut found = Vec::new();
        for (i, item) in candidates.iter().enumerate() {
            if matches(item, target) {
                found.push(Value::Number(i as f64));
                if limit != 0 && found.len() >= limit {
                    break;
                }
            }
        }
        found
    };

    match needle {
        Value::Number(_) | Value::Bool(_) => Value::list(find(needle)),
        // Characters not found are skipped when one match is requested.
        Value::String(s) => {
            let results: Vec<Value> = s
                .chars()
                .filter_map(|c| {
                    let found = find(&Value::string(c.to_string()));
                    if limit == 1 {
                        found.into_iter().next()
                    } else {
                        Some(Value::list(found))
                    }
                })
                .collect();
            Value::list(results)
        }
        Value::List(targets) => Value::list(
            targets
                .iter()
                .map(|t| {
                    let found = find(t);
                    if limit == 1 {
                        found.into_iter().next().unwrap_or_else(|| Value::list(vec![]))
                    } else {
                        Value::list(found)
                    }
                })
                .collect(),
        ),
        _ => Value::Undef,
    }
}

/// `rands(min, max, count, seed)`. The sequence differs from OpenSCAD's.
fn rands(ev: &mut Evaluator, args: &[Value]) -> Value {
    let (Some(min), Some(max), Some(count)) = (
        args.first().and_then(Value::as_number),
        args.get(1).and_then(Value::as_number),
        args.get(2).and_then(Value::as_number),
    ) else {
        return Value::Undef;
    };
    let mut state = match args.get(3).and_then(Value::as_number) {
        Some(seed) => seed.to_bits() ^ 0x2545_F491_4F6C_DD1D,
        None => ev.rng_state,
    };
    let mut next = || {
        // splitmix64
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let count = count.clamp(0.0, 1e6) as usize;
    let values = (0..count)
        .map(|_| {
            let unit = (next() >> 11) as f64 / (1u64 << 53) as f64;
            Value::Number(min + unit * (max - min))
        })
        .collect();
    if args.get(3).is_none() {
        ev.rng_state = state;
    }
    Value::list(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_trig() {
        assert_eq!(sin_degrees(30.0), 0.5);
        assert_eq!(sin_degrees(-90.0), -1.0);
        assert_eq!(cos_degrees(90.0), 0.0);
        assert_eq!(cos_degrees(420.0), 0.5);
        assert_eq!(tan_degrees(45.0), 1.0);
    }

    #[test]
    fn lookup_interpolates_and_clamps() {
        let table = Value::list(vec![
            Value::list(vec![0.0.into(), 0.0.into()]),
            Value::list(vec![10.0.into(), 100.0.into()]),
        ]);
        let at = |k: f64| lookup(&[k.into(), table.clone()]);
        assert_eq!(at(5.0), Value::Number(50.0));
        assert_eq!(at(-1.0), Value::Number(0.0));
        assert_eq!(at(11.0), Value::Number(100.0));
    }
}
