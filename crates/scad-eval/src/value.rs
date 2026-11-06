//! Runtime values and the operators defined on them.

use std::fmt::{self, Write as _};
use std::rc::Rc;

use scad_syntax::ast::{FunctionLiteral, Name};

use crate::scope::Scope;

#[derive(Clone, Default)]
pub enum Value {
    #[default]
    Undef,
    Bool(bool),
    Number(f64),
    String(Rc<str>),
    List(Rc<[Value]>),
    Range(RangeValue),
    Function(Rc<Closure>),
    Object(Rc<[(Name, Value)]>),
}

/// A function literal together with the scope it was created in.
pub struct Closure {
    pub literal: Rc<FunctionLiteral>,
    pub scope: Rc<Scope>,
    pub file: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangeValue {
    pub start: f64,
    pub step: f64,
    pub end: f64,
}

/// Ranges with this many elements or more are not iterated.
pub const MAX_RANGE_ELEMENTS: u64 = 1_000_000;
/// Element count reported for unbounded ranges (zero or infinite step).
const UNBOUNDED_RANGE: u64 = u32::MAX as u64;

impl RangeValue {
    /// Number of values, following OpenSCAD: an empty range when the step
    /// points away from the end, values computed as `start + i * step`,
    /// with a small tolerance so `[0:0.1:0.3]` includes 0.3.
    pub fn len(&self) -> u64 {
        let RangeValue { start, step, end } = *self;
        if start.is_nan() || step.is_nan() || end.is_nan() {
            return 0;
        }
        if (step < 0.0 && start < end) || (step > 0.0 && start > end) {
            return 0;
        }
        if start == end || step.is_infinite() {
            return 1;
        }
        if start.is_infinite() || end.is_infinite() || step == 0.0 {
            return UNBOUNDED_RANGE;
        }
        let steps = (end - start) / step;
        let steps = (steps + steps.abs() * 1e-12).floor();
        if steps >= UNBOUNDED_RANGE as f64 {
            UNBOUNDED_RANGE
        } else {
            steps as u64 + 1
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = f64> + '_ {
        (0..self.len()).map(|i| self.start + i as f64 * self.step)
    }
}

impl Value {
    pub fn number(n: f64) -> Self {
        Self::Number(n)
    }

    pub fn string(s: impl Into<Rc<str>>) -> Self {
        Self::String(s.into())
    }

    pub fn list(items: Vec<Value>) -> Self {
        Self::List(items.into())
    }

    pub fn object(entries: Vec<(Name, Value)>) -> Self {
        Self::Object(entries.into())
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Undef => "undefined",
            Self::Bool(_) => "bool",
            Self::Number(_) => "number",
            Self::String(_) => "string",
            Self::List(_) => "vector",
            Self::Range(_) => "range",
            Self::Function(_) => "function",
            Self::Object(_) => "object",
        }
    }

    pub fn is_undef(&self) -> bool {
        matches!(self, Self::Undef)
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Self::List(items) => Some(items),
            _ => None,
        }
    }

    /// Vector of exactly `N` numbers.
    pub fn as_vec<const N: usize>(&self) -> Option<[f64; N]> {
        let items = self.as_list()?;
        if items.len() != N {
            return None;
        }
        let mut out = [0.0; N];
        for (slot, item) in out.iter_mut().zip(items) {
            *slot = item.as_number()?;
        }
        Some(out)
    }

    /// OpenSCAD truthiness.
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Undef => false,
            Self::Bool(b) => *b,
            Self::Number(n) => *n != 0.0,
            Self::String(s) => !s.is_empty(),
            Self::List(items) => !items.is_empty(),
            Self::Object(entries) => !entries.is_empty(),
            Self::Range(_) | Self::Function(_) => true,
        }
    }

    /// Deep equality as used by `==`.
    pub fn equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Undef, Self::Undef) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::List(a), Self::List(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.equals(y))
            }
            // Unlike numbers, a range with NaN equals itself in OpenSCAD
            // (BOSL2's `is_nan()` relies on it to tell ranges from NaN).
            (Self::Range(a), Self::Range(b)) => {
                let same = |x: f64, y: f64| x == y || (x.is_nan() && y.is_nan());
                same(a.start, b.start) && same(a.step, b.step) && same(a.end, b.end)
            }
            (Self::Function(a), Self::Function(b)) => Rc::ptr_eq(a, b),
            (Self::Object(a), Self::Object(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .zip(b.iter())
                        .all(|((ka, va), (kb, vb))| ka == kb && va.equals(vb))
            }
            _ => false,
        }
    }

    /// Ordering for `<`, `<=`, `>`, `>=`; `None` when not comparable.
    /// Strings compare by UTF-8 bytes, vectors lexicographically.
    pub fn compare(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => a.partial_cmp(b),
            (Self::String(a), Self::String(b)) => Some(a.as_bytes().cmp(b.as_bytes())),
            (Self::Bool(a), Self::Bool(b)) => Some(a.cmp(b)),
            (Self::List(a), Self::List(b)) => {
                for (x, y) in a.iter().zip(b.iter()) {
                    match x.compare(y)? {
                        std::cmp::Ordering::Equal => {}
                        unequal => return Some(unequal),
                    }
                }
                Some(a.len().cmp(&b.len()))
            }
            // Ranges compare by begin, then step, then end.
            (Self::Range(a), Self::Range(b)) => {
                [a.start, a.step, a.end].partial_cmp(&[b.start, b.step, b.end])
            }
            _ => None,
        }
    }

    // ------------------------------------------------------- arithmetic

    pub fn neg(&self) -> Value {
        match self {
            Self::Number(n) => Self::Number(-n),
            Self::List(items) => map_list(items, |v| Some(v.neg())),
            _ => Self::Undef,
        }
    }

    pub fn add(&self, other: &Self) -> Value {
        self.elementwise(other, |a, b| a + b)
    }

    pub fn sub(&self, other: &Self) -> Value {
        self.elementwise(other, |a, b| a - b)
    }

    /// Numbers and same-length vectors (recursively) combine element-wise.
    fn elementwise(&self, other: &Self, op: fn(f64, f64) -> f64) -> Value {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => Self::Number(op(*a, *b)),
            (Self::List(a), Self::List(b)) => {
                let len = a.len().min(b.len());
                Self::list((0..len).map(|i| a[i].elementwise(&b[i], op)).collect())
            }
            _ => Self::Undef,
        }
    }

    pub fn mul(&self, other: &Self) -> Value {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => Self::Number(a * b),
            (Self::Number(_), Self::List(items)) => map_list(items, |v| Some(self.mul(v))),
            (Self::List(items), Self::Number(_)) => map_list(items, |v| Some(v.mul(other))),
            (Self::List(a), Self::List(b)) => multiply_lists(a, b),
            _ => Self::Undef,
        }
    }

    pub fn div(&self, other: &Self) -> Value {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => Self::Number(a / b),
            (Self::List(items), Self::Number(_)) => map_list(items, |v| Some(v.div(other))),
            (Self::Number(_), Self::List(items)) => map_list(items, |v| Some(self.div(v))),
            _ => Self::Undef,
        }
    }

    pub fn rem(&self, other: &Self) -> Value {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => Self::Number(a % b),
            _ => Self::Undef,
        }
    }

    pub fn pow(&self, other: &Self) -> Value {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => Self::Number(a.powf(*b)),
            _ => Self::Undef,
        }
    }

    /// Bitwise operators work on numbers truncated to 64-bit integers.
    pub fn bitwise(&self, other: &Self, op: fn(i64, i64) -> i64) -> Value {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => {
                Self::Number(op(to_int64(*a), to_int64(*b)) as f64)
            }
            _ => Self::Undef,
        }
    }

    pub fn bit_not(&self) -> Value {
        match self {
            Self::Number(n) => Self::Number(!to_int64(*n) as f64),
            _ => Self::Undef,
        }
    }

    /// `v[i]` on vectors/strings/ranges or `o["key"]` on objects.
    pub fn index(&self, index: &Self) -> Value {
        if let (Self::Object(entries), Self::String(key)) = (self, index) {
            return entries
                .iter()
                .find(|(k, _)| k.as_str() == key.as_ref())
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
        }
        // Indices are truncated toward zero, so -0.5 selects element 0.
        let Some(i) = index.as_number().map(f64::trunc) else {
            return Self::Undef;
        };
        if !i.is_finite() || i < 0.0 {
            return Self::Undef;
        }
        let i = i as usize;
        match self {
            Self::List(items) => items.get(i).cloned().unwrap_or_default(),
            Self::String(s) => s
                .chars()
                .nth(i)
                .map(|c| Self::string(c.to_string()))
                .unwrap_or_default(),
            Self::Range(r) => match i {
                0 => Self::Number(r.start),
                1 => Self::Number(r.step),
                2 => Self::Number(r.end),
                _ => Self::Undef,
            },
            _ => Self::Undef,
        }
    }

    /// `.x`, `.y`, `.z` on vectors and `.field` on objects (without swizzle).
    pub fn member(&self, name: &str) -> Value {
        self.member_with_swizzle(name, false)
    }

    /// Member lookup supporting optional `--enable=vector-swizzle` (`xyzw` / `rgba`, 1..=4 components).
    pub fn member_with_swizzle(&self, name: &str, swizzle: bool) -> Value {
        match self {
            Self::Object(entries) => entries
                .iter()
                .find(|(k, _)| k.as_str() == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_default(),
            Self::List(items) => {
                if !swizzle {
                    let i = match name {
                        "x" => 0,
                        "y" => 1,
                        "z" => 2,
                        _ => return Self::Undef,
                    };
                    return items.get(i).cloned().unwrap_or_default();
                }
                if name.is_empty() || name.len() > 4 {
                    return Self::Undef;
                }
                let indices: Option<Vec<usize>> =
                    if name.bytes().all(|b| matches!(b, b'x' | b'y' | b'z' | b'w')) {
                        Some(
                            name.bytes()
                                .map(|b| match b {
                                    b'x' => 0,
                                    b'y' => 1,
                                    b'z' => 2,
                                    _ => 3,
                                })
                                .collect(),
                        )
                    } else if name.bytes().all(|b| matches!(b, b'r' | b'g' | b'b' | b'a')) {
                        Some(
                            name.bytes()
                                .map(|b| match b {
                                    b'r' => 0,
                                    b'g' => 1,
                                    b'b' => 2,
                                    _ => 3,
                                })
                                .collect(),
                        )
                    } else {
                        None
                    };
                let Some(indices) = indices else {
                    return Self::Undef;
                };
                if indices.len() == 1 {
                    items.get(indices[0]).cloned().unwrap_or_default()
                } else {
                    Self::list(
                        indices
                            .into_iter()
                            .map(|i| items.get(i).cloned().unwrap_or_default())
                            .collect(),
                    )
                }
            }
            _ => Self::Undef,
        }
    }
}

/// Truncation to i64 as in C++: out-of-range values and NaN become i64::MIN.
fn to_int64(n: f64) -> i64 {
    const TWO_POW_63: f64 = 9_223_372_036_854_775_808.0;
    if n.is_finite() && n > -TWO_POW_63 && n < TWO_POW_63 {
        n as i64
    } else {
        i64::MIN
    }
}

/// Explains why `a * b` on two vectors is undefined, if it is.
pub fn vector_product_problem(a: &[Value], b: &[Value]) -> Option<String> {
    if a.is_empty() || b.is_empty() {
        return Some("Multiplication is undefined on empty vectors".into());
    }
    if is_numeric_vector(a) && is_numeric_vector(b) && a.len() != b.len() {
        return Some(format!(
            "vector*vector requires matching lengths ({} != {})",
            a.len(),
            b.len()
        ));
    }
    if is_matrix(a) && is_numeric_vector(b) {
        let columns = a[0].as_list().map_or(0, <[Value]>::len);
        if columns != b.len() {
            return Some(format!(
                "matrix*vector requires matrix column count to match vector length ({columns} != {})",
                b.len()
            ));
        }
    }
    if multiply_lists(a, b).is_undef() {
        return Some(format!(
            "undefined vector*vector multiplication where first elements are types {} and {}",
            a[0].type_name(),
            b[0].type_name()
        ));
    }
    None
}

fn map_list(items: &[Value], f: impl Fn(&Value) -> Option<Value>) -> Value {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match f(item) {
            Some(v) => out.push(v),
            None => return Value::Undef,
        }
    }
    Value::list(out)
}

fn is_numeric_vector(items: &[Value]) -> bool {
    items.iter().all(|v| matches!(v, Value::Number(_)))
}

fn is_matrix(items: &[Value]) -> bool {
    !items.is_empty()
        && items
            .iter()
            .all(|row| matches!(row, Value::List(r) if is_numeric_vector(r)))
}

/// Dot product, matrix × vector, vector × matrix and matrix × matrix.
fn multiply_lists(a: &[Value], b: &[Value]) -> Value {
    let dot = |x: &[Value], y: &[Value]| -> Option<f64> {
        if x.len() != y.len() {
            return None;
        }
        // Summed from +0 like OpenSCAD, so `-0 + -0` terms give 0 (Rust's
        // float `sum()` starts from -0).
        x.iter().zip(y).try_fold(0.0, |sum, (p, q)| {
            Some(sum + p.as_number()? * q.as_number()?)
        })
    };
    let column = |m: &[Value], j: usize| -> Option<Vec<Value>> {
        m.iter().map(|row| row.as_list()?.get(j).cloned()).collect()
    };

    if is_numeric_vector(a) && is_numeric_vector(b) {
        return dot(a, b).map_or(Value::Undef, Value::Number);
    }
    if is_matrix(a) && is_numeric_vector(b) {
        return a
            .iter()
            .map(|row| dot(row.as_list()?, b).map(Value::Number))
            .collect::<Option<Vec<_>>>()
            .map_or(Value::Undef, Value::list);
    }
    if is_numeric_vector(a) && is_matrix(b) {
        let cols = b[0].as_list().map_or(0, <[Value]>::len);
        if a.len() != b.len() {
            return Value::Undef;
        }
        return (0..cols)
            .map(|j| dot(a, &column(b, j)?).map(Value::Number))
            .collect::<Option<Vec<_>>>()
            .map_or(Value::Undef, Value::list);
    }
    if is_matrix(a) && is_matrix(b) {
        let cols = b[0].as_list().map_or(0, <[Value]>::len);
        return a
            .iter()
            .map(|row| {
                let row = row.as_list()?;
                (0..cols)
                    .map(|j| dot(row, &column(b, j)?).map(Value::Number))
                    .collect::<Option<Vec<_>>>()
                    .map(Value::list)
            })
            .collect::<Option<Vec<_>>>()
            .map_or(Value::Undef, Value::list);
    }
    Value::Undef
}

// ----------------------------------------------------------- formatting

/// Formats a number the way OpenSCAD prints it.
pub fn format_number(n: f64) -> String {
    geometry::format_number(n)
}

/// Strings in echo output and inside vectors are quoted but not escaped.
fn write_quoted(out: &mut String, s: &str) {
    out.push('"');
    out.push_str(s);
    out.push('"');
}

impl Value {
    /// Text produced by `str()`: strings appear raw at the top level.
    pub fn to_str_string(&self) -> String {
        match self {
            Self::String(s) => s.to_string(),
            other => other.to_repr(),
        }
    }

    /// Text used by `echo` and inside vectors: strings are quoted.
    pub fn to_repr(&self) -> String {
        let mut out = String::new();
        self.write_repr(&mut out);
        out
    }

    fn write_repr(&self, out: &mut String) {
        match self {
            Self::Undef => out.push_str("undef"),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Number(n) => out.push_str(&format_number(*n)),
            Self::String(s) => write_quoted(out, s),
            Self::List(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    item.write_repr(out);
                }
                out.push(']');
            }
            Self::Range(r) => {
                let _ = write!(
                    out,
                    "[{} : {} : {}]",
                    format_number(r.start),
                    format_number(r.step),
                    format_number(r.end)
                );
            }
            Self::Function(f) => {
                let _ = write!(
                    out,
                    "function({}) {}",
                    crate::print::params_to_string(&f.literal.params),
                    crate::print::expr_to_string(&f.literal.body)
                );
            }
            Self::Object(entries) => {
                if entries.is_empty() {
                    out.push_str("{ }");
                } else {
                    out.push_str("{ ");
                    for (i, (k, v)) in entries.iter().enumerate() {
                        if i > 0 {
                            out.push(' ');
                        }
                        let _ = write!(out, "{k} = ");
                        v.write_repr(out);
                        out.push(';');
                    }
                    out.push_str(" }");
                }
            }
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_repr())
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other)
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Self::Number(n)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Self::string(s)
    }
}

/// Positional and named arguments after evaluation.
#[derive(Debug, Clone, Default)]
pub struct Arguments {
    pub positional: Vec<Value>,
    pub named: Vec<(Name, Value)>,
    /// For each argument in source order, whether it was named.
    pub named_order: Vec<bool>,
}

impl Arguments {
    /// Every value in source order, names dropped.
    pub fn in_order(&self) -> Vec<Value> {
        let mut positional = self.positional.iter();
        let mut named = self.named.iter().map(|(_, v)| v);
        self.named_order
            .iter()
            .filter_map(|&is_named| {
                if is_named {
                    named.next()
                } else {
                    positional.next()
                }
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[f64]) -> Value {
        Value::list(items.iter().map(|&n| Value::Number(n)).collect())
    }

    #[test]
    fn range_lengths() {
        let r = |start, step, end| RangeValue { start, step, end }.len();
        assert_eq!(r(0.0, 1.0, 5.0), 6);
        assert_eq!(r(0.0, 2.0, 5.0), 3);
        assert_eq!(r(5.0, 1.0, 0.0), 0);
        assert_eq!(r(5.0, -1.0, 0.0), 6);
        assert_eq!(r(1.0, 1.0, 1.0), 1);
    }

    #[test]
    fn vector_math() {
        assert_eq!(v(&[1.0, 2.0]).add(&v(&[3.0, 4.0])), v(&[4.0, 6.0]));
        assert_eq!(v(&[1.0, 2.0]).mul(&Value::Number(2.0)), v(&[2.0, 4.0]));
        assert_eq!(v(&[1.0, 2.0]).mul(&v(&[3.0, 4.0])), Value::Number(11.0));
        let m = Value::list(vec![v(&[1.0, 2.0]), v(&[3.0, 4.0])]);
        assert_eq!(m.mul(&v(&[1.0, 1.0])), v(&[3.0, 7.0]));
        assert_eq!(v(&[1.0, 1.0]).mul(&m), v(&[4.0, 6.0]));
        assert_eq!(
            m.mul(&m),
            Value::list(vec![v(&[7.0, 10.0]), v(&[15.0, 22.0])])
        );
        assert!(Value::Number(1.0).add(&Value::string("a")).is_undef());
    }

    #[test]
    fn truthiness_and_equality() {
        assert!(!Value::Undef.is_truthy());
        assert!(!Value::Number(0.0).is_truthy());
        assert!(Value::string("x").is_truthy());
        assert!(!Value::list(vec![]).is_truthy());
        assert!(v(&[1.0, 2.0]).equals(&v(&[1.0, 2.0])));
        assert!(!Value::Number(1.0).equals(&Value::Bool(true)));
    }

    #[test]
    fn indexing() {
        let list = v(&[10.0, 20.0, 30.0]);
        assert_eq!(list.index(&Value::Number(1.7)), Value::Number(20.0));
        assert!(list.index(&Value::Number(-1.0)).is_undef());
        assert!(list.index(&Value::Number(3.0)).is_undef());
        assert_eq!(list.member("z"), Value::Number(30.0));
        assert_eq!(
            Value::string("abc").index(&Value::Number(1.0)),
            Value::string("b")
        );
    }
}
