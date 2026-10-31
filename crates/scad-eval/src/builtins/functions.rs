//! Builtin functions.

use std::cmp::Ordering;

use scad_syntax::ast::{Name, Span};

use crate::eval::{EvalOptions, Evaluator};
use crate::message::EvalResult;
use crate::value::{Arguments, Value};

const NAMES: &[&str] = &[
    "abs",
    "sign",
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "atan2",
    "floor",
    "ceil",
    "round",
    "ln",
    "log",
    "exp",
    "pow",
    "sqrt",
    "min",
    "max",
    "norm",
    "cross",
    "len",
    "concat",
    "lookup",
    "str",
    "chr",
    "ord",
    "search",
    "rands",
    "version",
    "version_num",
    "is_undef",
    "is_bool",
    "is_num",
    "is_string",
    "is_list",
    "is_function",
    "is_object",
    "parent_module",
];

pub fn is_disabled_experimental(name: &str, options: &EvalOptions) -> bool {
    match name {
        "object" | "has_key" => !options.object_function,
        "import" => !options.import_function,
        _ => false,
    }
}

pub fn exists(name: &str, options: &EvalOptions) -> bool {
    NAMES.contains(&name)
        || (options.object_function && matches!(name, "object" | "has_key"))
        || (options.import_function && name == "import")
}

/// OpenSCAD's version reported by `version()`: the nightly we target.
const VERSION: [f64; 3] = [2026.0, 10.0, 22.0];

/// Expected argument count for builtins that check it.
fn arity(name: &str) -> Option<usize> {
    match name {
        "abs" | "sign" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "floor" | "ceil"
        | "round" | "ln" | "exp" | "sqrt" | "norm" => Some(1),
        "atan2" | "pow" | "lookup" | "cross" | "log" | "has_key" => Some(2),
        _ => None,
    }
}

/// `"argument 0: expected number, found string ("a")"`
fn conversion_warning(
    ev: &mut Evaluator,
    name: &str,
    index: usize,
    expected: &str,
    found: &Value,
    span: Span,
) {
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
            Some(Value::Object(entries)) => Value::Number(entries.len() as f64),
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
        "search" => search(ev, a, span),
        "rands" => rands(ev, a, span),
        "version" => Value::list(VERSION.iter().map(|&n| Value::Number(n)).collect()),
        "version_num" => Value::Number(VERSION[0] * 10000.0 + VERSION[1] * 100.0 + VERSION[2]),
        "is_undef" => Value::Bool(a.first().is_none_or(Value::is_undef)),
        "is_bool" => Value::Bool(matches!(a.first(), Some(Value::Bool(_)))),
        "is_num" => Value::Bool(matches!(a.first(), Some(Value::Number(n)) if !n.is_nan())),
        "is_string" => Value::Bool(matches!(a.first(), Some(Value::String(_)))),
        "is_list" => Value::Bool(matches!(a.first(), Some(Value::List(_)))),
        "is_function" => Value::Bool(matches!(a.first(), Some(Value::Function(_)))),
        "is_object" => Value::Bool(matches!(a.first(), Some(Value::Object(_)))),
        "object" => construct_object(ev, &args, span),
        "has_key" => match (&a[0], &a[1]) {
            (Value::Object(entries), Value::String(key)) => {
                Value::Bool(entries.iter().any(|(k, _)| k.as_str() == key.as_ref()))
            }
            (other, _) if !matches!(other, Value::Object(_)) => {
                conversion_warning(ev, name, 0, "object", other, span);
                Value::Undef
            }
            (_, other) => {
                conversion_warning(ev, name, 1, "string", other, span);
                Value::Undef
            }
        },
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
            format!(
                "{name}() number of parameters does not match: expected at least 1{what}, found 0"
            ),
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
        let min = points
            .iter()
            .min_by(|a, b| a[0].total_cmp(&b[0]))
            .unwrap_or(first);
        return Value::Number(min[1]);
    }
    if high[0] < key {
        let max = points
            .iter()
            .max_by(|a, b| a[0].total_cmp(&b[0]))
            .unwrap_or(first);
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
fn search(ev: &mut Evaluator, args: &[Value], span: Span) -> Value {
    if !(2..=4).contains(&args.len()) {
        ev.warning(
            format!(
                "search() number of parameters does not match: expected between 2 and 4, found {}",
                args.len()
            ),
            span,
        );
        return Value::Undef;
    }
    let (needle, haystack) = (&args[0], &args[1]);
    let limit = args
        .get(2)
        .and_then(Value::as_number)
        .unwrap_or(1.0)
        .max(0.0) as usize;
    let column = args
        .get(3)
        .and_then(Value::as_number)
        .unwrap_or(0.0)
        .max(0.0) as usize;

    let candidates: Vec<Value> = match haystack {
        Value::List(items) => items.to_vec(),
        Value::String(s) => s.chars().map(|c| Value::string(c.to_string())).collect(),
        _ => return Value::Undef,
    };
    if let (Value::String(s), Value::List(items)) = (needle, haystack)
        && !s.is_empty()
    {
        for (i, item) in items.iter().enumerate() {
            let valid = match item {
                Value::List(row) => row.len() > column,
                Value::String(entry) => column == 0 && entry.chars().count() == 1,
                _ => false,
            };
            if !valid {
                ev.warning(
                    format!(
                        "Invalid entry in search vector at index {i}, required number of values in the entry: {}. Invalid entry: {}",
                        column + 1,
                        item.to_repr()
                    ),
                    span,
                );
                return Value::list(vec![]);
            }
        }
    }
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
                        found
                            .into_iter()
                            .next()
                            .unwrap_or_else(|| Value::list(vec![]))
                    } else {
                        Value::list(found)
                    }
                })
                .collect(),
        ),
        _ => Value::Undef,
    }
}

fn construct_object(ev: &mut Evaluator, args: &Arguments, span: Span) -> Value {
    let upsert = |entries: &mut Vec<(Name, Value)>, key: Name, value: Value| {
        if value.is_undef() {
            entries.retain(|(k, _)| k != &key);
        } else if let Some(slot) = entries.iter_mut().find(|(k, _)| k == &key) {
            slot.1 = value;
        } else {
            entries.push((key, value));
        }
    };
    let mut entries: Vec<(Name, Value)> = Vec::new();
    let mut pos_iter = args.positional.iter();
    let mut named_iter = args.named.iter();
    for &is_named in &args.named_order {
        if is_named {
            if let Some((name, value)) = named_iter.next() {
                upsert(&mut entries, name.clone(), value.clone());
            }
        } else if let Some(pos) = pos_iter.next() {
            match pos {
                Value::Object(src) => {
                    for (k, v) in src.iter() {
                        upsert(&mut entries, k.clone(), v.clone());
                    }
                }
                Value::List(pairs) => {
                    for pair in pairs.iter() {
                        match pair.as_list() {
                            Some([Value::String(k), v]) => {
                                upsert(&mut entries, Name::from(k.as_ref()), v.clone());
                            }
                            _ => {
                                ev.warning(
                                    format!("object() invalid entry {}", pair.to_repr()),
                                    span,
                                );
                            }
                        }
                    }
                }
                other => {
                    ev.warning(
                        format!(
                            "object() unexpected positional argument {}",
                            other.to_repr()
                        ),
                        span,
                    );
                }
            }
        }
    }
    Value::object(entries)
}

/// Standard 32-bit Mersenne Twister (`std::mt19937`), matching OpenSCAD's `rands()`.
#[derive(Clone)]
pub(crate) struct Mt19937 {
    mt: [u32; 624],
    index: usize,
}

impl Mt19937 {
    const N: usize = 624;
    const M: usize = 397;
    const MATRIX_A: u32 = 0x9908_B0DF;
    const UPPER_MASK: u32 = 0x8000_0000;
    const LOWER_MASK: u32 = 0x7FFF_FFFF;

    pub(crate) fn new(seed: u32) -> Self {
        let mut mt = [0u32; Self::N];
        mt[0] = seed;
        for i in 1..Self::N {
            let prev = mt[i - 1];
            mt[i] = 1_812_433_253_u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
        Self { mt, index: Self::N }
    }

    fn next_u32(&mut self) -> u32 {
        if self.index >= Self::N {
            for i in 0..Self::N {
                let y = (self.mt[i] & Self::UPPER_MASK)
                    | (self.mt[(i + 1) % Self::N] & Self::LOWER_MASK);
                self.mt[i] = self.mt[(i + Self::M) % Self::N]
                    ^ (y >> 1)
                    ^ if y & 1 != 0 { Self::MATRIX_A } else { 0 };
            }
            self.index = 0;
        }
        let mut y = self.mt[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9D2C_5680;
        y ^= (y << 15) & 0xEFC6_0000;
        y ^= y >> 18;
        y
    }

    /// `std::generate_canonical<double, 53>` over 32-bit `mt19937` draws.
    fn next_canonical(&mut self) -> f64 {
        let r0 = f64::from(self.next_u32());
        let r1 = f64::from(self.next_u32());
        let x = (r0 + r1 * 4_294_967_296.0) / 18_446_744_073_709_551_616.0;
        if x >= 1.0 {
            1.0 - f64::EPSILON / 2.0
        } else {
            x
        }
    }
}

/// `rands(min, max, count[, seed])` with exact `std::mt19937` semantics.
fn rands(ev: &mut Evaluator, args: &[Value], span: Span) -> Value {
    if args.len() != 3 && args.len() != 4 {
        ev.warning(
            format!(
                "rands() number of parameters does not match: expected 3 or 4, found {}",
                args.len()
            ),
            span,
        );
        return Value::Undef;
    }
    let (Some(min), Some(max), Some(count)) = (
        args.first().and_then(Value::as_number),
        args.get(1).and_then(Value::as_number),
        args.get(2).and_then(Value::as_number),
    ) else {
        return Value::Undef;
    };
    if !min.is_finite() || !max.is_finite() || !count.is_finite() {
        return Value::Undef;
    }
    let (min, max) = if min > max { (max, min) } else { (min, max) };
    let count = count.clamp(0.0, 1e6) as usize;
    let mut local_rng;
    let rng = match args.get(3).and_then(Value::as_number) {
        Some(seed) => {
            local_rng = Mt19937::new((seed.trunc() as i64) as u32);
            &mut local_rng
        }
        None => &mut ev.rng,
    };
    let values = (0..count)
        .map(|_| {
            if min == max {
                Value::Number(min)
            } else {
                let u = rng.next_canonical();
                Value::Number(u * (max - min) + min)
            }
        })
        .collect();
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
