//! Writes a geometry tree in OpenSCAD's `.csg` text format.

use std::fmt::Write as _;

use glam::{DMat4, DVec2, DVec3};

use crate::csg::{Node, NodeKind, OffsetKind, Segments};
use crate::format_number_csg as num;

/// Writes the top-level nodes. When some node carries the root modifier
/// (`!`), only the first such subtree is written, like OpenSCAD.
pub fn to_csg_text(top_level: &[Node]) -> String {
    let mut out = String::new();
    match find_root(top_level) {
        Some(root) => write_node(root, 0, true, &mut out),
        None => {
            for node in top_level {
                write_node(node, 0, false, &mut out);
            }
        }
    }
    out
}

fn find_root(nodes: &[Node]) -> Option<&Node> {
    nodes.iter().find_map(|node| {
        if node.modifiers.root {
            Some(node)
        } else {
            find_root(&node.children)
        }
    })
}

fn write_node(node: &Node, depth: usize, is_root: bool, out: &mut String) {
    // The modifier character comes before the indentation.
    if node.modifiers.background {
        out.push('%');
    } else if node.modifiers.highlight {
        out.push('#');
    } else if node.modifiers.root && !is_root {
        out.push('!');
    }
    out.extend(std::iter::repeat_n('\t', depth));
    out.push_str(&describe(&node.kind));
    if node.children.is_empty() {
        out.push_str(";\n");
    } else {
        out.push_str(" {\n");
        for child in &node.children {
            write_node(child, depth + 1, false, out);
        }
        out.extend(std::iter::repeat_n('\t', depth));
        out.push_str("}\n");
    }
}

fn vec2(v: DVec2) -> String {
    format!("[{}, {}]", num(v.x), num(v.y))
}

fn vec3(v: DVec3) -> String {
    format!("[{}, {}, {}]", num(v.x), num(v.y), num(v.z))
}

fn segments(s: &Segments) -> String {
    format!(
        "$fn = {}, $fa = {}, $fs = {}",
        num(s.fn_),
        num(s.fa),
        num(s.fs)
    )
}

fn bool_text(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

fn matrix(m: &DMat4) -> String {
    let rows: Vec<String> = (0..4)
        .map(|r| {
            let row = m.row(r);
            format!(
                "[{}, {}, {}, {}]",
                num(row.x),
                num(row.y),
                num(row.z),
                num(row.w)
            )
        })
        .collect();
    format!("[{}]", rows.join(", "))
}

fn index_lists(lists: &[Vec<usize>]) -> String {
    let inner: Vec<String> = lists
        .iter()
        .map(|l| {
            let items: Vec<String> = l.iter().map(usize::to_string).collect();
            format!("[{}]", items.join(", "))
        })
        .collect();
    format!("[{}]", inner.join(", "))
}

fn describe(kind: &NodeKind) -> String {
    match kind {
        NodeKind::Group { .. } => "group()".into(),
        NodeKind::Union => "union()".into(),
        NodeKind::Difference => "difference()".into(),
        NodeKind::Intersection => "intersection()".into(),
        NodeKind::Hull => "hull()".into(),
        NodeKind::Fill => "fill()".into(),
        NodeKind::Minkowski { convexity } => format!("minkowski(convexity = {})", num(*convexity)),
        NodeKind::Render { convexity } => format!("render(convexity = {})", num(*convexity)),
        NodeKind::Multmatrix(m) => format!("multmatrix({})", matrix(m)),
        NodeKind::Color { rgba } => format!(
            "color([{}, {}, {}, {}])",
            num(rgba[0]),
            num(rgba[1]),
            num(rgba[2]),
            num(rgba[3])
        ),
        NodeKind::Resize {
            size,
            auto,
            convexity,
        } => format!(
            "resize(newsize = [{},{},{}], auto = [{},{},{}], convexity = {})",
            num(size.x),
            num(size.y),
            num(size.z),
            u8::from(auto[0]),
            u8::from(auto[1]),
            u8::from(auto[2]),
            num(*convexity)
        ),
        NodeKind::LinearExtrude {
            height,
            direction,
            center,
            convexity,
            twist,
            slices,
            segments: segment_count,
            scale,
            tessellation,
        } => {
            let mut s = format!("linear_extrude(height = {}", num(*height));
            if let Some(v) = direction {
                let _ = write!(s, ", v = [ {}, {}, {}]", num(v.x), num(v.y), num(v.z));
            }
            if *center {
                s.push_str(", center = true");
            }
            if *twist != 0.0 {
                let _ = write!(s, ", twist = {}", num(*twist));
            }
            if let Some(slices) = slices {
                let _ = write!(s, ", slices = {}", num(*slices));
            }
            if let Some(count) = segment_count {
                let _ = write!(s, ", segments = {}", num(*count));
            }
            if *scale != DVec2::ONE {
                if scale.x == scale.y {
                    let _ = write!(s, ", scale = {}", num(scale.x));
                } else {
                    let _ = write!(s, ", scale = {}", vec2(*scale));
                }
            }
            let _ = write!(s, ", {}", segments(tessellation));
            if *convexity > 1.0 {
                let _ = write!(s, ", convexity = {}", num(*convexity));
            }
            s.push(')');
            s
        }
        NodeKind::RotateExtrude {
            angle,
            start,
            convexity,
            tessellation,
        } => format!(
            "rotate_extrude(angle = {}, start = {}, convexity = {}, {})",
            num(*angle),
            num(*start),
            num(*convexity),
            segments(tessellation)
        ),
        NodeKind::Offset { kind, tessellation } => match kind {
            OffsetKind::Radius(r) => format!("offset(r = {}, {})", num(*r), segments(tessellation)),
            OffsetKind::Delta { delta, chamfer } => format!(
                "offset(delta = {}, chamfer = {}, {})",
                num(*delta),
                bool_text(*chamfer),
                segments(tessellation)
            ),
        },
        NodeKind::Projection { cut, convexity } => format!(
            "projection(cut = {}, convexity = {})",
            bool_text(*cut),
            num(*convexity)
        ),
        NodeKind::Cube { size, center } => {
            format!(
                "cube(size = {}, center = {})",
                vec3(*size),
                bool_text(*center)
            )
        }
        NodeKind::Sphere {
            r,
            octa,
            tessellation,
        } => format!(
            "sphere({}, r = {}, style = \"{}\")",
            segments(tessellation),
            num(*r),
            if *octa { "octa" } else { "orig" }
        ),
        NodeKind::Cylinder {
            h,
            r1,
            r2,
            center,
            tessellation,
        } => format!(
            "cylinder({}, h = {}, r1 = {}, r2 = {}, center = {})",
            segments(tessellation),
            num(*h),
            num(*r1),
            num(*r2),
            bool_text(*center)
        ),
        NodeKind::Polyhedron {
            points,
            faces,
            convexity,
        } => {
            let points: Vec<String> = points.iter().map(|p| vec3(*p)).collect();
            format!(
                "polyhedron(points = [{}], faces = {}, convexity = {})",
                points.join(", "),
                index_lists(faces),
                num(*convexity)
            )
        }
        NodeKind::Square { size, center } => {
            format!(
                "square(size = {}, center = {})",
                vec2(*size),
                bool_text(*center)
            )
        }
        NodeKind::Circle { r, tessellation } => {
            format!("circle({}, r = {})", segments(tessellation), num(*r))
        }
        NodeKind::Polygon {
            points,
            paths,
            convexity,
        } => {
            let points: Vec<String> = points.iter().map(|p| vec2(*p)).collect();
            let paths = paths
                .as_deref()
                .map_or_else(|| "undef".to_string(), index_lists);
            format!(
                "polygon(points = [{}], paths = {}, convexity = {})",
                points.join(", "),
                paths,
                num(*convexity)
            )
        }
        NodeKind::Text {
            text,
            size,
            spacing,
            font,
            direction,
            language,
            script,
            halign,
            valign,
            tessellation,
        } => {
            let mut s = format!(
                "text(text = \"{text}\", size = {}, spacing = {}, font = \"{font}\", direction = \"{direction}\", language = \"{language}\"",
                num(*size),
                num(*spacing),
            );
            if let Some(script) = script {
                let _ = write!(s, ", script = \"{script}\"");
            }
            let _ = write!(
                s,
                ", halign = \"{halign}\", valign = \"{valign}\", {})",
                segments(tessellation)
            );
            s
        }
        NodeKind::Import {
            file,
            layer,
            origin,
            dpi,
            scale,
            center,
            convexity,
            tessellation,
            timestamp,
        } => {
            let mut s = format!("import(file = \"{file}\"");
            if let Some(layer) = layer {
                let _ = write!(s, ", layer = \"{layer}\"");
            }
            let _ = write!(s, ", origin = {}", vec2(*origin));
            if let Some(dpi) = dpi {
                let _ = write!(s, ", dpi = {}", num(*dpi));
            }
            let _ = write!(
                s,
                ", scale = {}, center = {}, convexity = {}, {}, timestamp = {timestamp})",
                num(*scale),
                bool_text(*center),
                num(*convexity),
                segments(tessellation)
            );
            s
        }
        NodeKind::Surface {
            file,
            center,
            invert,
            timestamp,
            ..
        } => format!(
            "surface(file = \"{file}\", center = {}, invert = {}, timestamp = {timestamp})",
            bool_text(*center),
            bool_text(*invert)
        ),
        NodeKind::Other { name, args } => format!("{name}({args})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csg::NodeModifiers;

    #[test]
    fn nested_output_with_modifiers() {
        let cube = Node::new(
            NodeKind::Cube {
                size: DVec3::ONE,
                center: false,
            },
            vec![],
        );
        let mut highlighted = cube.clone();
        highlighted.modifiers = NodeModifiers {
            highlight: true,
            ..Default::default()
        };
        let tree = vec![
            Node::new(
                NodeKind::Multmatrix(DMat4::from_translation(DVec3::X)),
                vec![highlighted],
            ),
            Node::group("for", vec![]),
        ];
        assert_eq!(
            to_csg_text(&tree),
            "multmatrix([[1, 0, 0, 1], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]) {\n\
             #\tcube(size = [1, 1, 1], center = false);\n\
             }\n\
             group();\n"
        );
    }
}
