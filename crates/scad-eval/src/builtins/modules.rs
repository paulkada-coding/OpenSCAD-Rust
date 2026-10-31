//! Builtin modules: they turn evaluated arguments into geometry nodes.

use geometry::csg::{Node, NodeKind, OffsetKind, Segments};
use geometry::glam::{DMat4, DVec2, DVec3, DVec4};
use scad_syntax::ast::Span;

use super::colors::parse_color;
use super::functions::{cos_degrees, sin_degrees};
use crate::eval::Evaluator;
use crate::message::EvalResult;
use crate::value::{Arguments, Value};

const NAMES: &[&str] = &[
    "cube", "sphere", "cylinder", "polyhedron", "square", "circle", "polygon", "translate",
    "rotate", "scale", "mirror", "multmatrix", "color", "resize", "offset", "linear_extrude",
    "rotate_extrude", "projection", "union", "difference", "intersection", "hull", "minkowski",
    "render", "group", "text", "import", "surface", "fill",
];

pub fn exists(name: &str) -> bool {
    NAMES.contains(&name)
}

/// Positional parameter order and extra named-only parameters.
fn signature(name: &str) -> Option<(&'static [&'static str], &'static [&'static str])> {
    Some(match name {
        "cube" | "square" => (&["size", "center"], &[]),
        "sphere" => (&["r"], &["d", "style"]),
        "cylinder" => (&["h", "r1", "r2", "center"], &["r", "d", "d1", "d2"]),
        "polyhedron" => (&["points", "faces", "convexity"], &["triangles"]),
        "circle" => (&["r"], &["d"]),
        "polygon" => (&["points", "paths", "convexity"], &[]),
        "translate" | "scale" | "mirror" => (&["v"], &[]),
        "rotate" => (&["a", "v"], &[]),
        "multmatrix" => (&["m"], &[]),
        "color" => (&["c", "alpha"], &[]),
        "resize" => (&["newsize", "auto", "convexity"], &[]),
        "offset" => (&["r", "delta", "chamfer"], &[]),
        "linear_extrude" => (
            &["height", "center", "convexity", "twist", "slices", "scale"],
            &["v", "segments"],
        ),
        "rotate_extrude" => (&["angle", "convexity"], &["start"]),
        "projection" => (&["cut", "convexity"], &[]),
        "render" | "minkowski" => (&["convexity"], &[]),
        "union" | "difference" | "intersection" | "hull" | "group" | "fill" => (&[], &[]),
        _ => return None,
    })
}

/// Arguments bound to parameters like a user module call: named ones
/// first, positional ones fill the remaining parameters in order.
struct Params<'a> {
    bound: Vec<(&'static str, &'a Value)>,
    named: &'a [(scad_syntax::ast::Name, Value)],
}

impl<'a> Params<'a> {
    fn new(args: &'a Arguments, order: &'static [&'static str]) -> Self {
        let named_names: Vec<&str> = args.named.iter().map(|(n, _)| n.as_str()).collect();
        let free = order.iter().filter(|p| !named_names.contains(p));
        let bound = free.copied().zip(args.positional.iter()).collect();
        Self {
            bound,
            named: &args.named,
        }
    }

    fn get(&self, name: &str) -> Option<&'a Value> {
        if let Some((_, value)) = self.named.iter().rev().find(|(n, _)| n == name) {
            return Some(value);
        }
        self.bound.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
    }

    fn number(&self, name: &str) -> Option<f64> {
        self.get(name).and_then(Value::as_number).filter(|n| n.is_finite())
    }

    fn bool(&self, name: &str, default: bool) -> bool {
        self.get(name).map_or(default, Value::is_truthy)
    }
}

/// `$fn`, `$fa`, `$fs` in effect, clamped like OpenSCAD.
fn tessellation(ev: &mut Evaluator, span: Span) -> Segments {
    let read = |ev: &Evaluator, name: &str, default: f64| {
        ev.special(name).as_number().unwrap_or(default)
    };
    let mut segments = Segments {
        fn_: read(ev, "$fn", 0.0),
        fa: read(ev, "$fa", 12.0),
        fs: read(ev, "$fs", 2.0),
    };
    if segments.fn_ < 0.0 {
        ev.warning("$fn negative - setting to 0", span);
        segments.fn_ = 0.0;
    }
    if segments.fs < 0.01 {
        ev.warning("$fs too small - clamping to 0.010000", span);
        segments.fs = 0.01;
    }
    if segments.fa < 0.01 {
        ev.warning("$fa too small - clamping to 0.010000", span);
        segments.fa = 0.01;
    }
    segments
}

/// Warnings for arguments that match no parameter of a builtin module.
fn check_arguments(ev: &mut Evaluator, name: &str, args: &Arguments, span: Span) {
    let Some((order, extra)) = signature(name) else {
        return;
    };
    if args.positional.len() > order.len() {
        ev.warning("Too many unnamed arguments supplied", span);
    }
    for (arg, _) in &args.named {
        let known = order.contains(&arg.as_str()) || extra.contains(&arg.as_str());
        if !known && !arg.starts_with('$') {
            ev.warning(format!("variable \"{arg}\" not specified as parameter"), span);
        }
    }
}

pub fn instantiate(
    ev: &mut Evaluator,
    name: &str,
    args: &Arguments,
    children: Vec<Node>,
    span: Span,
) -> EvalResult<Option<Node>> {
    let leaf = |kind| Ok(Some(Node::new(kind, Vec::new())));
    let node = |kind, children| Ok(Some(Node::new(kind, children)));
    check_arguments(ev, name, args, span);

    match name {
        "cube" => {
            let p = Params::new(args, &["size", "center"]);
            let size = match p.get("size") {
                None | Some(Value::Undef) => DVec3::ONE,
                Some(Value::Number(s)) => DVec3::splat(*s),
                Some(v) => match v.as_vec::<3>() {
                    Some(s) => DVec3::from_array(s),
                    None => {
                        ev.warning(
                            format!(
                                "Unable to convert cube(size={}, ...) parameter to a number or a vec3 of numbers",
                                v.to_repr()
                            ),
                            span,
                        );
                        DVec3::ONE
                    }
                },
            };
            leaf(NodeKind::Cube {
                size,
                center: p.bool("center", false),
            })
        }
        "sphere" => {
            let p = Params::new(args, &["r"]);
            let r = radius(ev, &p, span);
            let octa = match p.get("style") {
                None => false,
                Some(Value::String(s)) if s.as_ref() == "octa" => true,
                Some(Value::String(s)) if s.as_ref() == "orig" => false,
                Some(Value::String(s)) => {
                    ev.warning(
                        format!("sphere(style=\"{s}\") is not \"orig\" or \"octa\", using \"orig\""),
                        span,
                    );
                    false
                }
                Some(other) => {
                    ev.warning(
                        format!(
                            "(..., style={}) Invalid type: expected string, found {}",
                            other.to_repr(),
                            other.type_name()
                        ),
                        span,
                    );
                    false
                }
            };
            leaf(NodeKind::Sphere {
                r,
                octa,
                tessellation: tessellation(ev, span),
            })
        }
        "cylinder" => {
            let p = Params::new(args, &["h", "r1", "r2", "center"]);
            // A diameter wins over the matching radius, with a warning.
            let mut pick = |radius: &str, diameter: &str| match (p.number(radius), p.number(diameter)) {
                (Some(_), Some(d)) => {
                    ev.warning(
                        format!(
                            "Ignoring radius variable \"{radius}\" as diameter \"{diameter}\" is defined too."
                        ),
                        span,
                    );
                    Some(d / 2.0)
                }
                (None, Some(d)) => Some(d / 2.0),
                (r, None) => r,
            };
            let r = pick("r", "d");
            let r1 = pick("r1", "d1");
            let r2 = pick("r2", "d2");
            if r.is_some() && (r1.is_some() || r2.is_some()) {
                ev.warning("Cylinder parameters ambiguous", span);
            }
            leaf(NodeKind::Cylinder {
                h: p.number("h").unwrap_or(1.0),
                r1: r1.or(r).unwrap_or(1.0),
                r2: r2.or(r).unwrap_or(1.0),
                center: p.bool("center", false),
                tessellation: tessellation(ev, span),
            })
        }
        "polyhedron" => {
            let p = Params::new(args, &["points", "faces", "convexity"]);
            let points = p
                .get("points")
                .and_then(Value::as_list)
                .map(|items| items.iter().filter_map(Value::as_vec::<3>).map(DVec3::from_array).collect())
                .unwrap_or_default();
            let faces = p
                .get("faces")
                .or_else(|| p.get("triangles"))
                .map(index_lists)
                .unwrap_or_default();
            leaf(NodeKind::Polyhedron {
                points,
                faces,
                convexity: p.number("convexity").unwrap_or(1.0),
            })
        }
        "square" => {
            let p = Params::new(args, &["size", "center"]);
            let size = match p.get("size") {
                Some(Value::Number(s)) => DVec2::splat(*s),
                Some(v) => v.as_vec::<2>().map_or(DVec2::ONE, DVec2::from_array),
                None => DVec2::ONE,
            };
            leaf(NodeKind::Square {
                size,
                center: p.bool("center", false),
            })
        }
        "circle" => {
            let p = Params::new(args, &["r"]);
            let r = radius(ev, &p, span);
            leaf(NodeKind::Circle {
                r,
                tessellation: tessellation(ev, span),
            })
        }
        "polygon" => {
            let p = Params::new(args, &["points", "paths", "convexity"]);
            let points = p
                .get("points")
                .and_then(Value::as_list)
                .map(|items| items.iter().filter_map(Value::as_vec::<2>).map(DVec2::from_array).collect())
                .unwrap_or_default();
            leaf(NodeKind::Polygon {
                points,
                paths: p.get("paths").filter(|v| !v.is_undef()).map(index_lists),
                convexity: p.number("convexity").unwrap_or(1.0),
            })
        }
        "translate" => {
            let p = Params::new(args, &["v"]);
            // Adding to zero turns -0 into 0, as OpenSCAD's matrix sum does.
            let v = p.get("v").and_then(vec3_lenient).unwrap_or(DVec3::ZERO) + DVec3::ZERO;
            node(NodeKind::Multmatrix(DMat4::from_translation(v)), children)
        }
        "rotate" => {
            let p = Params::new(args, &["a", "v"]);
            node(NodeKind::Multmatrix(rotation(p.get("a"), p.get("v"))), children)
        }
        "scale" => {
            let p = Params::new(args, &["v"]);
            let v = match p.get("v") {
                Some(Value::Number(s)) => DVec3::splat(*s),
                Some(v) => vec3_lenient(v)
                    .map(|s| {
                        // A 2-vector leaves z unscaled.
                        if v.as_list().is_some_and(|l| l.len() == 2) {
                            DVec3::new(s.x, s.y, 1.0)
                        } else {
                            s
                        }
                    })
                    .unwrap_or(DVec3::ONE),
                None => DVec3::ONE,
            };
            node(NodeKind::Multmatrix(DMat4::from_scale(v)), children)
        }
        "mirror" => {
            let p = Params::new(args, &["v"]);
            let n = p.get("v").and_then(vec3_lenient).unwrap_or(DVec3::X);
            node(NodeKind::Multmatrix(mirror(n)), children)
        }
        "multmatrix" => {
            let p = Params::new(args, &["m"]);
            let m = p.get("m").map_or(DMat4::IDENTITY, matrix);
            node(NodeKind::Multmatrix(m), children)
        }
        "color" => {
            let p = Params::new(args, &["c", "alpha"]);
            let alpha = p.number("alpha");
            let mut rgba = match p.get("c") {
                Some(Value::String(s)) => match parse_color(s) {
                    Some(c) => c,
                    None => {
                        ev.warning(format!("Unable to parse color \"{s}\""), span);
                        [-1.0; 4]
                    }
                },
                // Missing channels default to 1.
                Some(v) => match v.as_list() {
                    Some(items) if !items.is_empty() => {
                        let c = |i: usize| items.get(i).and_then(Value::as_number).unwrap_or(1.0);
                        [c(0), c(1), c(2), c(3)]
                    }
                    _ => [-1.0; 4],
                },
                None => [-1.0; 4],
            };
            if let Some(alpha) = alpha {
                rgba[3] = alpha;
            }
            node(NodeKind::Color { rgba }, children)
        }
        "resize" => {
            let p = Params::new(args, &["newsize", "auto", "convexity"]);
            let size = p.get("newsize").and_then(vec3_lenient).unwrap_or(DVec3::ZERO);
            let auto = match p.get("auto") {
                Some(Value::List(items)) => {
                    let b = |i: usize| items.get(i).is_some_and(Value::is_truthy);
                    [b(0), b(1), b(2)]
                }
                Some(v) => [v.is_truthy(); 3],
                None => [false; 3],
            };
            node(
                NodeKind::Resize {
                    size,
                    auto,
                    convexity: p.number("convexity").unwrap_or(0.0),
                },
                children,
            )
        }
        "offset" => {
            let p = Params::new(args, &["r", "delta", "chamfer"]);
            let kind = match (p.number("r"), p.number("delta")) {
                (Some(r), Some(_)) => {
                    ev.warning("Ignoring \"delta\" argument as \"r\" is defined too.", span);
                    OffsetKind::Radius(r)
                }
                (Some(r), None) => OffsetKind::Radius(r),
                (None, Some(delta)) => OffsetKind::Delta {
                    delta,
                    chamfer: p.bool("chamfer", false),
                },
                (None, None) => OffsetKind::Radius(1.0),
            };
            node(
                NodeKind::Offset {
                    kind,
                    tessellation: tessellation(ev, span),
                },
                children,
            )
        }
        "linear_extrude" => {
            let p = Params::new(
                args,
                &["height", "center", "convexity", "twist", "slices", "scale"],
            );
            let scale = match p.get("scale") {
                Some(Value::Number(s)) => DVec2::splat(*s),
                Some(v) => v.as_vec::<2>().map_or(DVec2::ONE, DVec2::from_array),
                None => DVec2::ONE,
            };
            // `v` gives the direction (and, without `height`, the length).
            let v = p
                .get("v")
                .and_then(Value::as_vec::<3>)
                .map(DVec3::from_array)
                .filter(|v| v.length() > 0.0);
            let height = p
                .number("height")
                .or_else(|| v.map(DVec3::length))
                .unwrap_or(100.0)
                .max(0.0);
            let direction = v
                .map(DVec3::normalize)
                .filter(|d| d.x != 0.0 || d.y != 0.0);
            node(
                NodeKind::LinearExtrude {
                    height,
                    direction,
                    center: p.bool("center", false),
                    convexity: p.number("convexity").unwrap_or(1.0),
                    twist: p.number("twist").unwrap_or(0.0),
                    slices: p.number("slices"),
                    segments: p.number("segments"),
                    scale,
                    tessellation: tessellation(ev, span),
                },
                children,
            )
        }
        "rotate_extrude" => {
            let p = Params::new(args, &["angle", "convexity"]);
            let angle = p.number("angle");
            // Without an explicit angle the sweep starts at 180Â°, as in
            // OpenSCAD's classic behavior.
            let start = p
                .number("start")
                .unwrap_or(if angle.is_some() { 0.0 } else { 180.0 });
            node(
                NodeKind::RotateExtrude {
                    angle: angle.unwrap_or(360.0).clamp(-360.0, 360.0),
                    start,
                    convexity: p.number("convexity").unwrap_or(2.0).max(2.0),
                    tessellation: tessellation(ev, span),
                },
                children,
            )
        }
        "fill" => node(NodeKind::Fill, children),
        "projection" => {
            let p = Params::new(args, &["cut", "convexity"]);
            node(
                NodeKind::Projection {
                    cut: p.bool("cut", false),
                    convexity: p.number("convexity").unwrap_or(0.0),
                },
                children,
            )
        }
        "union" => node(NodeKind::Union, children),
        "difference" => node(NodeKind::Difference, children),
        "intersection" => node(NodeKind::Intersection, children),
        "hull" => node(NodeKind::Hull, children),
        "minkowski" => {
            let p = Params::new(args, &["convexity"]);
            node(
                NodeKind::Minkowski {
                    convexity: p.number("convexity").unwrap_or(0.0),
                },
                children,
            )
        }
        "render" => {
            let p = Params::new(args, &["convexity"]);
            node(
                NodeKind::Render {
                    convexity: p.number("convexity").unwrap_or(1.0),
                },
                children,
            )
        }
        "group" => Ok(Some(Node::group("group", children))),
        _ => {
            // Not meshed yet: keep the call with its arguments.
            let mut parts: Vec<String> = args.positional.iter().map(Value::to_repr).collect();
            parts.extend(
                args.named
                    .iter()
                    .map(|(n, v)| format!("{n} = {}", v.to_repr())),
            );
            node(
                NodeKind::Other {
                    name: name.to_string(),
                    args: parts.join(", "),
                },
                children,
            )
        }
    }
}

/// `r` or `d` (diameter wins, with a warning when both are given).
fn radius(ev: &mut Evaluator, p: &Params<'_>, span: Span) -> f64 {
    match (p.number("r"), p.number("d")) {
        (Some(_), Some(d)) => {
            ev.warning(
                "Ignoring radius variable \"r\" as diameter \"d\" is defined too.",
                span,
            );
            d / 2.0
        }
        (None, Some(d)) => d / 2.0,
        (Some(r), None) => r,
        (None, None) => 1.0,
    }
}

fn index_lists(value: &Value) -> Vec<Vec<usize>> {
    value
        .as_list()
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_list)
                .map(|row| {
                    row.iter()
                        .filter_map(Value::as_number)
                        .map(|n| n.max(0.0) as usize)
                        .collect()
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A 2- or 3-vector of numbers as a 3D vector (z = 0 for 2-vectors).
fn vec3_lenient(value: &Value) -> Option<DVec3> {
    if let Some(v) = value.as_vec::<3>() {
        return Some(DVec3::from_array(v));
    }
    value.as_vec::<2>().map(|[x, y]| DVec3::new(x, y, 0.0))
}

/// `rotate(a)`, `rotate([x, y, z])` and `rotate(a, v)`.
fn rotation(a: Option<&Value>, v: Option<&Value>) -> DMat4 {
    let axis_angle = |angle: f64, axis: DVec3| -> DMat4 {
        let Some(u) = axis.try_normalize() else {
            return DMat4::IDENTITY;
        };
        let (s, c) = (sin_degrees(angle), cos_degrees(angle));
        let t = 1.0 - c;
        DMat4::from_cols(
            DVec4::new(t * u.x * u.x + c, t * u.x * u.y + s * u.z, t * u.x * u.z - s * u.y, 0.0),
            DVec4::new(t * u.x * u.y - s * u.z, t * u.y * u.y + c, t * u.y * u.z + s * u.x, 0.0),
            DVec4::new(t * u.x * u.z + s * u.y, t * u.y * u.z - s * u.x, t * u.z * u.z + c, 0.0),
            DVec4::W,
        )
    };
    match a {
        Some(Value::Number(angle)) => {
            let axis = v.and_then(vec3_lenient).unwrap_or(DVec3::Z);
            axis_angle(*angle, axis)
        }
        Some(value @ Value::List(items)) => {
            let angles = vec3_lenient(value).unwrap_or_else(|| {
                DVec3::new(items.first().and_then(Value::as_number).unwrap_or(0.0), 0.0, 0.0)
            });
            // Rotate about X, then Y, then Z.
            axis_angle(angles.z, DVec3::Z)
                * axis_angle(angles.y, DVec3::Y)
                * axis_angle(angles.x, DVec3::X)
        }
        _ => DMat4::IDENTITY,
    }
}

fn mirror(n: DVec3) -> DMat4 {
    let Some(n) = n.try_normalize() else {
        return DMat4::IDENTITY;
    };
    let col = |axis: DVec3| (axis - 2.0 * n * n.dot(axis)).extend(0.0);
    DMat4::from_cols(col(DVec3::X), col(DVec3::Y), col(DVec3::Z), DVec4::W)
}

/// A row-major 4Ã—4 (or 3Ã—4) nested list as a matrix.
fn matrix(value: &Value) -> DMat4 {
    let mut m = DMat4::IDENTITY;
    if let Some(rows) = value.as_list() {
        for (r, row) in rows.iter().take(4).enumerate() {
            if let Some(cells) = row.as_list() {
                for (c, cell) in cells.iter().take(4).enumerate() {
                    if let Some(x) = cell.as_number() {
                        m.col_mut(c)[r] = x;
                    }
                }
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations_are_exact_at_right_angles() {
        let m = rotation(Some(&Value::Number(90.0)), None);
        let p = m.transform_point3(DVec3::X);
        assert_eq!(p, DVec3::Y);
        let euler = Value::list(vec![90.0.into(), 0.0.into(), 90.0.into()]);
        let p = rotation(Some(&euler), None).transform_point3(DVec3::Y);
        assert_eq!(p, DVec3::Z);
    }

    #[test]
    fn mirror_and_matrix() {
        assert_eq!(mirror(DVec3::X).transform_point3(DVec3::new(2.0, 1.0, 0.0)), DVec3::new(-2.0, 1.0, 0.0));
        let rows = Value::list(vec![
            Value::list(vec![1.0.into(), 0.0.into(), 0.0.into(), 5.0.into()]),
            Value::list(vec![0.0.into(), 1.0.into(), 0.0.into(), 0.0.into()]),
            Value::list(vec![0.0.into(), 0.0.into(), 1.0.into(), 0.0.into()]),
        ]);
        assert_eq!(matrix(&rows).transform_point3(DVec3::ZERO), DVec3::new(5.0, 0.0, 0.0));
    }
}
