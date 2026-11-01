//! Replays snippets whose geometry was recorded from OpenSCAD (see
//! `fixtures/openscad.tsv`): our vertices must be bit-identical (same
//! count and hash), volumes and bounds equal to 1e-9. OpenSCAD is not
//! needed to run this test.

use std::path::Path;

use geometry::Mesh;
use geometry::glam::DVec3;
use mesher::{RenderOptions, Shape};
use scad_eval::{EvalOptions, Evaluator};

/// Cases whose vertices are known to differ slightly from OpenSCAD (the
/// test then checks volume and bounds only), with the reason.
const APPROXIMATE: &[(&str, &str)] = &[
    ("octa_def", "octa sphere interior points"),
    ("le_tw_default", "a few ulp-level vertex differences"),
    ("mink2d_nc", "2D minkowski of a non-convex polygon"),
];

fn render(code: &str) -> (Option<Mesh>, Vec<String>) {
    let mut ev = Evaluator::new(EvalOptions::default());
    let (nodes, err) = ev.run(Path::new("fixture.scad"), code);
    assert!(err.is_none(), "evaluation failed for {code}");
    let out = mesher::render(&nodes, &RenderOptions::default()).expect("render");
    let mesh = match out.shape {
        Shape::Solid(m) => Some(m),
        Shape::Empty => None,
        Shape::Planar { .. } => panic!("2D result for {code}"),
    };
    (mesh, out.warnings)
}

fn vertex_hash(m: &Mesh) -> u64 {
    let mut keys: Vec<[u64; 3]> = m
        .vertices
        .iter()
        .map(|&p| {
            let p = p + DVec3::ZERO;
            [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]
        })
        .collect();
    keys.sort_unstable();
    keys.dedup();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for k in keys.iter().flatten() {
        for byte in k.to_le_bytes() {
            h ^= u64::from(byte);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

fn parse_vec(s: &str) -> DVec3 {
    let v: Vec<f64> = s.split(',').map(|x| x.parse().unwrap()).collect();
    DVec3::new(v[0], v[1], v[2])
}

#[test]
fn matches_openscad_fixtures() {
    let text = include_str!("fixtures/openscad.tsv");
    let mut failures = Vec::new();
    let mut checked = 0;
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let fields: Vec<&str> = line.split('\t').collect();
        let (name, code) = (fields[0], *fields.last().unwrap());
        let (mesh, _) = render(code);
        checked += 1;
        if fields[1] == "empty" {
            if mesh.as_ref().is_some_and(|m| !m.is_empty()) {
                failures.push(format!("{name}: expected empty"));
            }
            continue;
        }
        let Some(mesh) = mesh else {
            failures.push(format!("{name}: empty result"));
            continue;
        };
        let verts: usize = fields[1].parse().unwrap();
        let hash = u64::from_str_radix(fields[2], 16).unwrap();
        let volume: f64 = fields[3].parse().unwrap();
        let (min, max) = (parse_vec(fields[4]), parse_vec(fields[5]));
        let b = mesh.bounds();
        if (mesh.volume() - volume).abs() > 1e-9 * volume.abs().max(1.0) {
            failures.push(format!("{name}: volume {} != {volume}", mesh.volume()));
        }
        if (b.min - min).abs().max_element() > 1e-9 || (b.max - max).abs().max_element() > 1e-9 {
            failures.push(format!("{name}: bounds {:?} != {min:?}..{max:?}", b));
        }
        if APPROXIMATE.iter().any(|(n, _)| *n == name) {
            continue;
        }
        let ours = Mesh {
            vertices: mesh.vertices.clone(),
            triangles: Vec::new(),
        };
        let distinct = {
            let mut v: Vec<[u64; 3]> = ours
                .vertices
                .iter()
                .map(|p| (*p + DVec3::ZERO).to_array().map(f64::to_bits))
                .collect();
            v.sort_unstable();
            v.dedup();
            v.len()
        };
        if distinct != verts || vertex_hash(&ours) != hash {
            failures.push(format!(
                "{name}: vertices differ ({distinct} vs {verts}, hash {:016x} vs {hash:016x})",
                vertex_hash(&ours)
            ));
        }
    }
    assert!(checked > 50, "fixture file not read");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn mixed_dimensions_warn_like_openscad() {
    let (mesh, warnings) = render("cube(5); square(3);");
    assert!(mesh.is_some());
    assert_eq!(
        warnings,
        [
            "Mixing 2D and 3D objects is not supported",
            "Ignoring 2D child object for 3D operation"
        ]
    );
}
