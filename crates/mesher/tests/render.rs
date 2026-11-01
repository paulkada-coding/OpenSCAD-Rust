//! Behavior of `mesher::render` beyond plain geometry: modifiers,
//! provenance, diagnostics and cancellation.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use mesher::{Level, RenderError, RenderOptions, RenderOutput, Shape};
use scad_eval::{EvalOptions, Evaluator};

fn render_with(code: &str, options: &RenderOptions) -> Result<RenderOutput, RenderError> {
    let mut ev = Evaluator::new(EvalOptions::default());
    let (nodes, err) = ev.run(Path::new("t.scad"), code);
    assert!(err.is_none());
    mesher::render(&nodes, options)
}

fn render(code: &str) -> RenderOutput {
    render_with(code, &RenderOptions::default()).expect("render")
}

fn solid(out: &RenderOutput) -> &geometry::Mesh {
    match &out.shape {
        Shape::Solid(m) => m,
        other => panic!("not a solid: {other:?}"),
    }
}

#[test]
fn provenance_maps_triangles_to_primitives() {
    let code = "cube(2);\ntranslate([2, 1, 1]) sphere(1, $fn = 12);";
    let out = render(code);
    let mesh = solid(&out);
    assert_eq!(out.triangle_sources.len(), mesh.triangles.len());
    // Both primitives appear, and every triangle is attributed.
    let mut used: Vec<u32> = out.triangle_sources.clone();
    used.sort_unstable();
    used.dedup();
    assert_eq!(used.len(), 2, "{used:?}");
    assert!(used.iter().all(|&i| (i as usize) < out.sources.len()));
    let snippets: Vec<&str> = out
        .sources
        .iter()
        .map(|s| &code[s.start as usize..s.end as usize])
        .collect();
    assert!(snippets.iter().any(|s| s.starts_with("cube")));
    assert!(snippets.iter().any(|s| s.contains("sphere")));
    // A lone primitive is attributed too.
    let lone = render("cylinder(h = 1, r = 1);");
    assert!(lone.triangle_sources.iter().all(|&i| i == 0));
}

#[test]
fn modifiers() {
    // `%` is shown but not part of the result.
    let out = render("cube(1); %translate([5, 0, 0]) cube(1);");
    assert!((solid(&out).volume() - 1.0).abs() < 1e-12);
    assert_eq!(out.background.len(), 1);
    match &out.background[0] {
        Shape::Solid(m) => assert_eq!(m.bounds().min.x, 5.0),
        other => panic!("{other:?}"),
    }
    // `#` is part of the result and also highlighted, in world space.
    let out = render("translate([2, 0, 0]) #cube(1); cube(1);");
    assert!((solid(&out).volume() - 2.0).abs() < 1e-12);
    assert_eq!(out.highlighted.len(), 1);
    match &out.highlighted[0] {
        Shape::Solid(m) => assert_eq!(m.bounds().min.x, 2.0),
        other => panic!("{other:?}"),
    }
    // `!` keeps only that subtree, without its parents' transforms.
    let out = render("cube(10); translate([5, 0, 0]) !cube(1);");
    let m = solid(&out);
    assert!((m.volume() - 1.0).abs() < 1e-12);
    assert_eq!(m.bounds().min.x, 0.0);
}

#[test]
fn planar_results() {
    let out = render("difference() { square(4, center = true); circle(1, $fn = 8); }");
    let Shape::Planar { outlines, fill } = &out.shape else {
        panic!("expected 2D");
    };
    assert_eq!(outlines.len(), 2);
    let area: f64 = fill
        .triangles()
        .map(|[a, b, c]| (b - a).cross(c - a).z / 2.0)
        .sum();
    let octagon = 2.0 * 2f64.sqrt();
    assert!((area - (16.0 - octagon)).abs() < 1e-6, "{area}");
}

#[test]
fn diagnostics_like_openscad() {
    let out = render("rotate_extrude() translate([-1, 0]) square(2);");
    assert!(out.shape.is_empty());
    assert_eq!(out.diagnostics[0].level, Level::Error);
    assert_eq!(
        out.diagnostics[0].text,
        "Children of rotate_extrude() may not lie across the Y axis (Range of X coords for all children [-1.00 : 1.00])"
    );
    // An open polyhedron is dropped from a union, with OpenSCAD's messages.
    let out = render(
        "union() { polyhedron([[0,0,0],[1,0,0],[0,1,0],[0,0,1]], [[0,1,2],[0,3,1],[0,2,3]]); translate([3,0,0]) cube(1); }",
    );
    assert!((solid(&out).volume() - 1.0).abs() < 1e-12);
    assert_eq!(
        out.warnings,
        ["PolySet -> Manifold conversion failed: NotManifold"]
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.level == Level::Error && d.text == "[manifold] Input mesh is not closed!")
    );
    // Warnings carry the node that raised them.
    let code = "cube(5);\nsquare(3);";
    let out = render(code);
    assert_eq!(out.warning_sources.len(), out.warnings.len());
    let s = out.warning_sources[0].expect("source");
    assert!(code[s.start as usize..s.end as usize].starts_with("square"));
}

#[test]
fn cancellation() {
    let flag = Arc::new(AtomicBool::new(true));
    let options = RenderOptions { cancel: Some(flag) };
    let result = render_with("cube(1);", &options);
    assert!(matches!(result, Err(RenderError::Cancelled)));
}
