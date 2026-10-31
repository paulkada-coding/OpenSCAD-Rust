//! Integration tests against analytic values.

use std::f64::consts::PI;
use std::sync::Arc;

use manifold::glam::{DAffine2, DAffine3, DMat3, DVec2, DVec3};
use manifold::{
    Aabb, CrossSection, Error, FillRule, JoinType, Manifold, Mesh, OpType, Rect, extrude, revolve,
};

const EPS: f64 = 1e-9;

fn assert_close(actual: f64, expected: f64, tol: f64) {
    assert!(
        (actual - expected).abs() <= tol,
        "expected {expected}, got {actual} (tolerance {tol})"
    );
}

/// Unit cube [0,1]^3 as 12 outward-facing triangles.
fn cube_mesh() -> Mesh {
    let p = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
    let quads = [
        [p(0., 0., 0.), p(0., 1., 0.), p(1., 1., 0.), p(1., 0., 0.)],
        [p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.)],
        [p(0., 0., 0.), p(1., 0., 0.), p(1., 0., 1.), p(0., 0., 1.)],
        [p(0., 1., 0.), p(0., 1., 1.), p(1., 1., 1.), p(1., 1., 0.)],
        [p(0., 0., 0.), p(0., 0., 1.), p(0., 1., 1.), p(0., 1., 0.)],
        [p(1., 0., 0.), p(1., 1., 0.), p(1., 1., 1.), p(1., 0., 1.)],
    ];
    Mesh::from_triangle_soup(
        quads
            .iter()
            .flat_map(|[a, b, c, d]| [[*a, *b, *c], [*a, *c, *d]]),
    )
}

fn unit_cube() -> Manifold {
    Manifold::from_mesh(&cube_mesh()).expect("valid cube")
}

fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<DVec2> {
    vec![
        DVec2::new(x0, y0),
        DVec2::new(x1, y0),
        DVec2::new(x1, y1),
        DVec2::new(x0, y1),
    ]
}

fn signed_area(contour: &[DVec2]) -> f64 {
    let n = contour.len();
    (0..n)
        .map(|i| contour[i].perp_dot(contour[(i + 1) % n]))
        .sum::<f64>()
        / 2.0
}

// ----- Mesh conversion and errors -----

#[test]
fn cube_round_trip() {
    let cube = unit_cube();
    assert_eq!(cube.status(), Ok(()));
    assert!(!cube.is_empty());
    assert_eq!(cube.num_vert(), 8);
    assert_eq!(cube.num_tri(), 12);
    assert_eq!(cube.num_edge(), 18);
    assert_eq!(cube.genus(), 0);
    assert_close(cube.volume(), 1.0, EPS);
    assert_close(cube.surface_area(), 6.0, EPS);
    assert_eq!(
        cube.bounds(),
        Aabb {
            min: DVec3::ZERO,
            max: DVec3::ONE
        }
    );
    assert!(cube.epsilon() > 0.0 && cube.epsilon() < 1e-6);

    let mesh = cube.to_mesh();
    assert_eq!(mesh.vertices.len(), 8);
    assert_eq!(mesh.triangles.len(), 12);
    // Same winding convention: positive volume, same area.
    assert_close(mesh.volume(), 1.0, EPS);
    assert_close(mesh.surface_area(), 6.0, EPS);
    // Every input vertex survives exactly.
    for v in &cube_mesh().vertices {
        assert!(mesh.vertices.contains(v));
    }
    // And the output converts back.
    assert_close(Manifold::from_mesh(&mesh).unwrap().volume(), 1.0, EPS);
}

#[test]
fn empty_mesh_is_empty_solid() {
    let m = Manifold::from_mesh(&Mesh::new()).unwrap();
    assert!(m.is_empty());
    assert_eq!(m.volume(), 0.0);
    assert!(m.bounds().is_empty());
    assert!(m.to_mesh().is_empty());
}

#[test]
fn open_mesh_is_not_manifold() {
    let mut mesh = cube_mesh();
    mesh.triangles.pop();
    assert_eq!(Manifold::from_mesh(&mesh).unwrap_err(), Error::NotManifold);
}

#[test]
fn bad_index_and_nan_are_reported() {
    let mut mesh = cube_mesh();
    mesh.triangles[0][0] = 99;
    assert_eq!(
        Manifold::from_mesh(&mesh).unwrap_err(),
        Error::VertexIndexOutOfBounds
    );

    let mut mesh = cube_mesh();
    mesh.vertices[0].x = f64::NAN;
    assert_eq!(
        Manifold::from_mesh(&mesh).unwrap_err(),
        Error::NonFiniteVertex
    );
}

#[test]
fn duplicated_vertices_need_merge() {
    // Unweld one corner: give the first triangle its own copy of vertex 0.
    let mut mesh = cube_mesh();
    let v0 = mesh.triangles[0][0];
    mesh.vertices.push(mesh.vertices[v0 as usize]);
    mesh.triangles[0][0] = (mesh.vertices.len() - 1) as u32;
    assert_eq!(Manifold::from_mesh(&mesh).unwrap_err(), Error::NotManifold);
    let merged = Manifold::from_mesh_merged(&mesh).unwrap();
    assert_close(merged.volume(), 1.0, EPS);
}

#[test]
fn error_messages_are_user_facing() {
    assert!(Error::NotManifold.to_string().contains("non manifold"));
}

// ----- Booleans -----

#[test]
fn boolean_volumes() {
    let a = unit_cube();
    let b = a.translate(DVec3::new(0.5, 0.25, 0.0));
    // Overlap: 0.5 * 0.75 * 1.
    assert_close(a.intersection(&b).volume(), 0.375, EPS);
    assert_close(a.union(&b).volume(), 2.0 - 0.375, EPS);
    assert_close(a.difference(&b).volume(), 1.0 - 0.375, EPS);
    assert_close(a.boolean(&b, OpType::Subtract).volume(), 0.625, EPS);
    // A hole through a cube: genus 1.
    let big = Manifold::cube(DVec3::splat(3.0), true);
    let bar = Manifold::cube(DVec3::new(1.0, 1.0, 5.0), true);
    let ring = big.difference(&bar);
    assert_close(ring.volume(), 27.0 - 3.0, EPS);
    assert_eq!(ring.genus(), 1);
}

#[test]
fn batch_booleans() {
    let cube = unit_cube();
    let disjoint: Vec<Manifold> = (0..10)
        .map(|i| cube.translate(DVec3::new(2.0 * f64::from(i), 0.0, 0.0)))
        .collect();
    let all = Manifold::batch_union(&disjoint);
    assert_close(all.volume(), 10.0, EPS);
    assert_eq!(all.decompose().len(), 10);

    // Overlapping chain: each cube shifted by half.
    let chain: Vec<Manifold> = (0..5)
        .map(|i| cube.translate(DVec3::new(0.5 * f64::from(i), 0.0, 0.0)))
        .collect();
    assert_close(Manifold::batch_union(&chain).volume(), 3.0, EPS);
    assert_close(
        Manifold::batch_boolean(&chain, OpType::Intersect).volume(),
        0.0,
        EPS,
    );
    assert_close(
        Manifold::batch_boolean(&chain[..2], OpType::Intersect).volume(),
        0.5,
        EPS,
    );
    // Subtract: first minus all the others.
    let base = Manifold::cube(DVec3::new(4.0, 1.0, 1.0), false);
    let cutters = [base.clone(), cube.clone(), cube.translate(DVec3::X * 3.0)];
    assert_close(
        Manifold::batch_boolean(&cutters, OpType::Subtract).volume(),
        2.0,
        EPS,
    );
    assert!(Manifold::batch_union(&[]).is_empty());
}

#[test]
fn empty_results() {
    let a = unit_cube();
    let far = a.translate(DVec3::splat(10.0));
    let none = a.intersection(&far);
    assert_eq!(none.status(), Ok(()));
    assert!(none.is_empty());
    assert_eq!(none.num_tri(), 0);
    assert_eq!(none.bounds(), Aabb::EMPTY);
    assert!(none.to_mesh().is_empty());
    assert!(a.difference(&a).is_empty());
    assert!(Manifold::empty().union(&Manifold::empty()).is_empty());
    assert!(Manifold::default().is_empty());
}

#[test]
fn invalid_status_propagates() {
    let bad = Manifold::cube(DVec3::new(-1.0, 1.0, 1.0), false);
    assert_eq!(bad.status(), Err(Error::InvalidConstruction));
    assert!(bad.is_empty());
    let combined = unit_cube().union(&bad);
    assert_eq!(combined.status(), Err(Error::InvalidConstruction));
}

// ----- Transforms -----

#[test]
fn transforms() {
    let cube = unit_cube();
    let m = DAffine3::from_scale_rotation_translation(
        DVec3::new(2.0, 3.0, 4.0),
        manifold::glam::DQuat::from_rotation_z(0.3),
        DVec3::new(5.0, -1.0, 2.0),
    );
    let t = cube.transform(&m);
    assert_close(t.volume(), 24.0, 1e-9);
    assert_close(t.bounds().min.z, 2.0, 1e-12);
    assert_close(t.bounds().max.z, 6.0, 1e-12);

    let s = cube.scale(DVec3::new(2.0, 1.0, 1.0)).translate(DVec3::Z);
    assert_eq!(
        s.bounds(),
        Aabb {
            min: DVec3::new(0.0, 0.0, 1.0),
            max: DVec3::new(2.0, 1.0, 2.0)
        }
    );

    // Mirroring keeps a valid, positive-volume solid.
    let mirrored = cube.mirror(DVec3::X);
    assert_eq!(mirrored.status(), Ok(()));
    assert_close(mirrored.volume(), 1.0, EPS);
    assert_close(mirrored.bounds().min.x, -1.0, EPS);
    assert_close(mirrored.to_mesh().volume(), 1.0, EPS);
    let flip = cube.transform(&DAffine3::from_mat3(DMat3::from_diagonal(DVec3::new(
        1.0, -1.0, 1.0,
    ))));
    assert_close(flip.volume(), 1.0, EPS);

    // Exact quarter turn.
    let r = cube.rotate(DVec3::new(0.0, 0.0, 90.0));
    assert_eq!(
        r.bounds(),
        Aabb {
            min: DVec3::new(-1.0, 0.0, 0.0),
            max: DVec3::new(0.0, 1.0, 1.0)
        }
    );
}

// ----- Hulls, Minkowski, splits -----

#[test]
fn hulls() {
    let cube = unit_cube();
    let far = cube.translate(DVec3::new(3.0, 0.0, 0.0));
    assert_close(
        Manifold::hull_of(&[cube.clone(), far.clone()]).volume(),
        4.0,
        EPS,
    );
    assert_close(cube.union(&far).hull().volume(), 4.0, EPS);

    let tetra = [DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::Z, DVec3::splat(0.1)];
    assert_close(Manifold::hull_of_points(&tetra).volume(), 1.0 / 6.0, EPS);
    // Coplanar points: a degenerate, zero-volume solid (not an empty one).
    let flat = [DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::new(1.0, 1.0, 0.0)];
    let flat_hull = Manifold::hull_of_points(&flat);
    assert_eq!(flat_hull.status(), Ok(()));
    assert_eq!(flat_hull.volume(), 0.0);
}

#[test]
fn minkowski() {
    let cube = Manifold::cube(DVec3::ONE, true);
    let sum = cube.minkowski_sum(&cube);
    assert_close(sum.volume(), 8.0, 1e-9);
    // Non-convex operand: an L shape grown by a small cube.
    let l = Manifold::cube(DVec3::new(2.0, 1.0, 1.0), false)
        .union(&Manifold::cube(DVec3::new(1.0, 2.0, 1.0), false));
    let grown = l.minkowski_sum(&Manifold::cube(DVec3::splat(0.2), true));
    assert_eq!(grown.status(), Ok(()));
    // L grown by 0.1 on every side: prism of the grown L outline (area
    // 2.2 * 1.2 + 1.2 * 1.0 = 3.84) times height 1.2.
    assert_close(grown.volume(), 3.84 * 1.2, 1e-6);
    let eroded = Manifold::cube(DVec3::splat(4.0), true)
        .minkowski_difference(&Manifold::cube(DVec3::splat(2.0), true));
    assert_close(eroded.volume(), 8.0, 1e-6);
}

#[test]
fn splits() {
    let cube = Manifold::cube(DVec3::splat(2.0), true);
    let (above, below) = cube.split_by_plane(DVec3::Z, 0.5);
    assert_close(above.volume(), 2.0, EPS);
    assert_close(below.volume(), 6.0, EPS);
    assert_close(above.bounds().min.z, 0.5, EPS);
    let trimmed = cube.trim_by_plane(DVec3::new(0.0, 0.0, -3.0), 0.0);
    assert_close(trimmed.volume(), 4.0, EPS);
    assert_close(trimmed.bounds().max.z, 0.0, EPS);

    let (inside, outside) = cube.split(&unit_cube());
    assert_close(inside.volume(), 1.0, EPS);
    assert_close(outside.volume(), 7.0, EPS);
}

// ----- 3D -> 2D -----

#[test]
fn slice_and_project() {
    let cube = Manifold::cube(DVec3::new(2.0, 3.0, 4.0), true);
    let s = cube.slice(1.0);
    assert_close(s.area(), 6.0, EPS);
    assert_eq!(
        s.bounds(),
        Rect {
            min: DVec2::new(-1.0, -1.5),
            max: DVec2::new(1.0, 1.5)
        }
    );
    assert!(cube.slice(10.0).is_empty());

    let rotated = cube.rotate(DVec3::new(90.0, 0.0, 0.0));
    assert_close(rotated.project().area(), 8.0, EPS);

    let ring = Manifold::cube(DVec3::splat(3.0), true)
        .difference(&Manifold::cube(DVec3::new(1.0, 1.0, 5.0), true));
    let section = ring.slice(0.0);
    assert_close(section.area(), 8.0, EPS);
    assert_eq!(section.num_contour(), 2);
}

// ----- CrossSection -----

#[test]
fn cross_section_booleans() {
    let a = CrossSection::square(DVec2::splat(2.0), false);
    let b = CrossSection::from_polygons(&[square(1.0, 1.0, 3.0, 3.0)], FillRule::Positive);
    assert_close(a.area(), 4.0, EPS);
    assert_close(a.union(&b).area(), 7.0, EPS);
    assert_close(a.difference(&b).area(), 3.0, EPS);
    assert_close(a.intersection(&b).area(), 1.0, EPS);
    assert_close(a.boolean(&b, OpType::Intersect).area(), 1.0, EPS);

    let row: Vec<CrossSection> = (0..4)
        .map(|i| a.translate(DVec2::new(f64::from(i), 0.0)))
        .collect();
    assert_close(CrossSection::batch_union(&row).area(), 10.0, EPS);
    assert_close(
        CrossSection::batch_boolean(&row, OpType::Subtract).area(),
        2.0,
        EPS,
    );
    assert!(CrossSection::batch_union(&[]).is_empty());
    assert!(a.intersection(&a.translate(DVec2::splat(5.0))).is_empty());
}

#[test]
fn fill_rules() {
    let outer = square(0.0, 0.0, 10.0, 10.0);
    let inner = square(2.0, 2.0, 8.0, 8.0); // same (CCW) orientation
    let contours = [outer.clone(), inner.clone()];
    // OpenSCAD polygon() semantics: nested paths alternate.
    let even_odd = CrossSection::from_polygons(&contours, FillRule::EvenOdd);
    assert_close(even_odd.area(), 64.0, EPS);
    assert_eq!(even_odd.num_contour(), 2);
    assert_close(
        CrossSection::from_polygons(&contours, FillRule::NonZero).area(),
        100.0,
        EPS,
    );
    // Clockwise outline: empty under Positive, full under Negative.
    let cw: Vec<DVec2> = outer.iter().rev().copied().collect();
    assert!(CrossSection::from_polygons(std::slice::from_ref(&cw), FillRule::Positive).is_empty());
    assert_close(
        CrossSection::from_polygons(&[cw], FillRule::Negative).area(),
        100.0,
        EPS,
    );

    // Output orientation: outlines CCW, holes CW.
    let polys = even_odd.to_polygons();
    assert_eq!(polys.len(), 2);
    let mut areas: Vec<f64> = polys.iter().map(|c| signed_area(c)).collect();
    areas.sort_by(f64::total_cmp);
    assert_close(areas[0], -36.0, EPS);
    assert_close(areas[1], 100.0, EPS);
    // Round trip.
    assert_close(
        CrossSection::from_polygons(&polys, FillRule::Positive).area(),
        64.0,
        EPS,
    );
    // Self-intersecting bow tie: two triangles of area 1/4 each.
    let bow = [
        DVec2::ZERO,
        DVec2::new(1.0, 1.0),
        DVec2::new(1.0, 0.0),
        DVec2::new(0.0, 1.0),
    ];
    assert_close(
        CrossSection::from_polygons(&[bow], FillRule::EvenOdd).area(),
        0.5,
        EPS,
    );
}

#[test]
fn offsets() {
    let sq = CrossSection::square(DVec2::splat(10.0), true);
    // Rounded corners: four quarter circles, approximated by a 256-gon.
    let n = 256.0;
    let circle_area = 0.5 * n * (2.0 * PI / n).sin();
    let round = sq.offset(1.0, JoinType::Round, 2.0, 256);
    assert_close(round.area(), 100.0 + 40.0 + circle_area, 1e-2);
    assert_close(sq.offset(1.0, JoinType::Miter, 2.0, 0).area(), 144.0, 1e-9);
    // Square join: corners cut at distance 1 from the original vertex.
    let squared = sq.offset(1.0, JoinType::Square, 2.0, 0).area();
    assert!(squared > 140.0 && squared < 144.0, "{squared}");
    let bevel = sq.offset(1.0, JoinType::Bevel, 2.0, 0).area();
    assert_close(bevel, 144.0 - 4.0 * 0.5, 1e-9);
    // Shrinking.
    assert_close(sq.offset(-1.0, JoinType::Miter, 2.0, 0).area(), 64.0, 1e-9);
    assert!(sq.offset(-6.0, JoinType::Round, 2.0, 32).is_empty());
}

#[test]
fn cross_section_misc() {
    let tri = CrossSection::from_polygons(
        &[[DVec2::ZERO, DVec2::new(4.0, 0.0), DVec2::new(0.0, 4.0)]],
        FillRule::EvenOdd,
    );
    assert_close(tri.area(), 8.0, EPS);
    assert_eq!(tri.num_vert(), 3);
    assert_close(tri.hull().area(), 8.0, EPS);
    let far = tri.translate(DVec2::new(10.0, 0.0));
    assert_close(CrossSection::hull_of(&[tri.clone(), far]).area(), 48.0, EPS);
    let pts = [
        DVec2::ZERO,
        DVec2::new(2.0, 0.0),
        DVec2::new(2.0, 2.0),
        DVec2::new(0.0, 2.0),
        DVec2::ONE,
    ];
    assert_close(CrossSection::hull_of_points(&pts).area(), 4.0, EPS);

    let m = DAffine2::from_scale_angle_translation(DVec2::new(2.0, 3.0), 0.7, DVec2::new(1.0, 1.0));
    assert_close(tri.transform(&m).area(), 48.0, 1e-9);
    assert_close(tri.scale(DVec2::new(-1.0, 1.0)).area(), 8.0, EPS);
    assert_close(tri.mirror(DVec2::X).bounds().min.x, -4.0, EPS);
    assert_eq!(
        tri.rotate(90.0).bounds(),
        Rect {
            min: DVec2::new(-4.0, 0.0),
            max: DVec2::new(0.0, 4.0)
        }
    );
    let circle = CrossSection::circle(1.0, 64);
    assert_eq!(circle.num_vert(), 64);
    assert_close(circle.area(), 0.5 * 64.0 * (2.0 * PI / 64.0).sin(), 1e-9);
    // Collinear point removal.
    let with_mid = CrossSection::from_polygons(
        &[[
            DVec2::ZERO,
            DVec2::new(1.0, 0.0),
            DVec2::new(2.0, 0.0),
            DVec2::new(2.0, 2.0),
            DVec2::new(0.0, 2.0),
        ]],
        FillRule::Positive,
    );
    assert_eq!(with_mid.simplify(1e-6).num_vert(), 4);

    let empty = CrossSection::empty();
    assert!(empty.is_empty());
    assert_eq!(empty.area(), 0.0);
    assert_eq!(empty.bounds(), Rect::EMPTY);
    assert!(empty.to_polygons().is_empty());
    assert!(CrossSection::default().is_empty());
}

// ----- Extrude and revolve -----

#[test]
fn extrusions() {
    let sq = CrossSection::square(DVec2::splat(2.0), true);
    let prism = extrude(&sq, 3.0, 0, 0.0, DVec2::ONE);
    assert_close(prism.volume(), 12.0, EPS);
    assert_eq!(
        prism.bounds(),
        Aabb {
            min: DVec3::new(-1.0, -1.0, 0.0),
            max: DVec3::new(1.0, 1.0, 3.0)
        }
    );
    // Pyramid and frustum are exact.
    assert_close(extrude(&sq, 3.0, 0, 0.0, DVec2::ZERO).volume(), 4.0, EPS);
    let frustum = extrude(&sq, 3.0, 0, 0.0, DVec2::splat(2.0));
    assert_close(frustum.volume(), 1.0 * (4.0 + 16.0 + 8.0), EPS);
    // Twist: each division keeps its area, but the twisted side quads are
    // split along one diagonal, so the volume converges to area * height
    // only as O(1/n).
    let twisted = extrude(&sq, 3.0, 64, 90.0, DVec2::ONE);
    assert_eq!(twisted.status(), Ok(()));
    assert_close(twisted.volume(), 12.0, 0.1);
    let finer = extrude(&sq, 3.0, 256, 90.0, DVec2::ONE);
    assert!((finer.volume() - 12.0).abs() < (twisted.volume() - 12.0).abs());
    // Positive twist is counter-clockwise: the top corner (1, 1) ends at (-1, 1).
    let top = twisted.slice(3.0 - 1e-9);
    assert_close(top.area(), 4.0, 1e-6);
    let quarter = extrude(
        &CrossSection::square(DVec2::new(2.0, 1.0), false),
        1.0,
        8,
        90.0,
        DVec2::ONE,
    );
    let b = quarter.slice(1.0 - 1e-9).bounds();
    assert_close(b.min.x, -1.0, 1e-6);
    assert_close(b.max.y, 2.0, 1e-6);
    // Twist + scale together.
    let both = extrude(&sq, 2.0, 32, 45.0, DVec2::splat(0.5));
    assert_close(both.slice(2.0 - 1e-9).area(), 1.0, 1e-6);

    // Degenerate inputs give the empty solid, not an error.
    assert!(extrude(&CrossSection::empty(), 1.0, 0, 0.0, DVec2::ONE).is_empty());
    assert!(extrude(&sq, 0.0, 0, 0.0, DVec2::ONE).is_empty());
    assert!(extrude(&sq, f64::NAN, 0, 0.0, DVec2::ONE).is_empty());
}

#[test]
fn revolutions() {
    // Rectangle [1,2] x [0,1] around the axis: a tube.
    let rect = CrossSection::from_polygons(&[square(1.0, 0.0, 2.0, 1.0)], FillRule::Positive);
    let n = 128;
    let polygon_factor = 0.5 * f64::from(n) * (2.0 * PI / f64::from(n)).sin();
    let tube = revolve(&rect, n, 360.0);
    assert_eq!(tube.status(), Ok(()));
    assert_close(tube.volume(), polygon_factor * (4.0 - 1.0), 1e-9);
    assert_eq!(tube.genus(), 1);
    // The profile's Y axis becomes Z.
    assert_close(tube.bounds().max.z, 1.0, EPS);
    assert_close(tube.bounds().max.x, 2.0, EPS);

    // Torus: R = 3, r = 1, V = 2 pi^2 R r^2.
    let circle = CrossSection::circle(1.0, 128).translate(DVec2::new(3.0, 0.0));
    let torus = revolve(&circle, 256, 360.0);
    assert_close(
        torus.volume(),
        2.0 * PI * PI * 3.0,
        0.01 * 2.0 * PI * PI * 3.0,
    );

    // Partial sweep: the segment count applies to the swept angle, so this
    // quarter tube has n wedges of 90/n degrees.
    let quarter = revolve(&rect, n, 90.0);
    let wedges = f64::from(n) * 0.5 * (PI / 2.0 / f64::from(n)).sin();
    assert_close(quarter.volume(), wedges * 3.0, 1e-9);
    assert_eq!(quarter.status(), Ok(()));

    // Parts at x < 0 are dropped; nothing at x > 0 gives the empty solid.
    let centered = CrossSection::square(DVec2::splat(2.0), true);
    let cyl = revolve(&centered, n, 360.0);
    assert_close(cyl.volume(), polygon_factor * 1.0 * 2.0, 1e-9);
    let left = CrossSection::from_polygons(&[square(-2.0, 0.0, -1.0, 1.0)], FillRule::Positive);
    assert!(revolve(&left, n, 360.0).is_empty());
    assert!(revolve(&CrossSection::empty(), n, 360.0).is_empty());
}

// ----- Triangulation and provenance -----

#[test]
fn triangulation() {
    // Square with a square hole: 8 triangles covering area 64.
    let outer = square(0.0, 0.0, 10.0, 10.0);
    let hole: Vec<DVec2> = square(2.0, 2.0, 8.0, 8.0).into_iter().rev().collect();
    let contours = [outer, hole];
    let all: Vec<DVec2> = contours.iter().flatten().copied().collect();
    let tris = manifold::triangulate(&contours, -1.0);
    assert_eq!(tris.len(), 8);
    let area: f64 = tris
        .iter()
        .map(|t| (all[t[1]] - all[t[0]]).perp_dot(all[t[2]] - all[t[0]]) / 2.0)
        .sum();
    assert_close(area, 64.0, EPS);
    // Every triangle is counter-clockwise.
    for t in &tris {
        assert!((all[t[1]] - all[t[0]]).perp_dot(all[t[2]] - all[t[0]]) > 0.0);
    }
    assert!(manifold::triangulate::<Vec<DVec2>>(&[], -1.0).is_empty());
}

#[test]
fn original_ids_survive_booleans() {
    let a = unit_cube().as_original();
    let b = Manifold::cube(DVec3::ONE, false)
        .translate(DVec3::new(0.5, 0.5, 0.5))
        .as_original();
    let (ida, idb) = (a.original_id(), b.original_id());
    assert!(ida >= 0 && idb >= 0 && ida != idb);
    let u = a.union(&b);
    assert_eq!(u.original_id(), -1);
    let (mesh, runs) = u.to_mesh_with_runs();
    assert!(!runs.is_empty());
    assert_eq!(runs.last().unwrap().triangles.end, mesh.triangles.len());
    let ids: Vec<u32> = runs.iter().map(|r| r.original_id).collect();
    assert!(ids.contains(&(ida as u32)) && ids.contains(&(idb as u32)));
    // Runs tile the triangle list in order.
    for pair in runs.windows(2) {
        assert_eq!(pair[0].triangles.end, pair[1].triangles.start);
    }
}

#[test]
fn integer_clipper() {
    use manifold::clipper::{self, ClipOp, Join};
    let sq = |x0: i64, y0: i64, x1: i64, y1: i64| vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    let area = |paths: &[clipper::Path64]| -> f64 {
        paths
            .iter()
            .map(|p| {
                (0..p.len())
                    .map(|i| {
                        let (a, b) = (p[i], p[(i + 1) % p.len()]);
                        (a[0] as f64) * (b[1] as f64) - (b[0] as f64) * (a[1] as f64)
                    })
                    .sum::<f64>()
                    / 2.0
            })
            .sum()
    };
    let a = [sq(0, 0, 10, 10)];
    let b = [sq(5, 5, 15, 15)];
    let u = clipper::boolean(&a, &b, ClipOp::Union, FillRule::NonZero);
    assert_eq!(area(&u), 175.0);
    let d = clipper::boolean(&a, &b, ClipOp::Difference, FillRule::NonZero);
    assert_eq!(area(&d), 75.0);
    let i = clipper::boolean(&a, &b, ClipOp::Intersection, FillRule::NonZero);
    assert_eq!(area(&i), 25.0);
    // Exact integer vertices survive.
    assert!(i[0].contains(&[5, 5]) && i[0].contains(&[10, 10]));
    // Nested same-orientation contours under EvenOdd: a frame with a hole.
    let frame = clipper::boolean(
        &[sq(0, 0, 10, 10), sq(2, 2, 8, 8)],
        &[],
        ClipOp::Union,
        FillRule::EvenOdd,
    );
    assert_eq!(frame.len(), 2);
    assert_eq!(area(&frame), 64.0);

    let grown = clipper::offset(&a, 2.0, Join::Miter, 2.0, 0.0);
    assert_eq!(area(&grown), 196.0);
    let round = clipper::offset(&[sq(0, 0, 1000, 1000)], 100.0, Join::Round, 2.0, 1.0);
    assert!(area(&round) > 1_400_000.0 && area(&round) < 1_400_000.0 + 31_416.0);

    // Swept along the boundary: a 12x12 ring with an 8x8 hole.
    let m = clipper::minkowski_sum(&sq(-1, -1, 1, 1), &sq(0, 0, 10, 10));
    assert_eq!(area(&m), 144.0 - 64.0);
}

// ----- Ownership and threads -----

#[test]
fn clones_are_independent() {
    let a = unit_cube();
    let b = a.clone();
    drop(a);
    assert_close(b.volume(), 1.0, EPS);
    let cs = CrossSection::square(DVec2::ONE, false);
    let copies: Vec<CrossSection> = (0..100).map(|_| cs.clone()).collect();
    drop(cs);
    assert_close(CrossSection::batch_union(&copies).area(), 1.0, EPS);
}

#[test]
fn send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Manifold>();
    assert_send_sync::<CrossSection>();

    // Concurrent queries on the same lazily evaluated result.
    let cubes: Vec<Manifold> = (0..20)
        .map(|i| unit_cube().translate(DVec3::new(0.5 * f64::from(i), 0.0, 0.0)))
        .collect();
    let lazy = Arc::new(Manifold::batch_union(&cubes).difference(&unit_cube()));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let shared = Arc::clone(&lazy);
            std::thread::spawn(move || (shared.volume(), shared.to_mesh().triangles.len()))
        })
        .collect();
    let results: Vec<(f64, usize)> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for (volume, tris) in &results {
        assert_close(*volume, 10.5 - 1.0, EPS);
        assert_eq!(*tris, results[0].1);
    }

    // Independent work on separate threads.
    let handles: Vec<_> = (0..8)
        .map(|i| {
            std::thread::spawn(move || {
                let s = Manifold::sphere(1.0 + f64::from(i), 32);
                s.difference(&s.scale(DVec3::splat(0.5))).volume()
            })
        })
        .collect();
    for h in handles {
        assert!(h.join().unwrap() > 0.0);
    }
}

#[test]
fn cxx_exceptions_become_panics() {
    // Clipper2 rejects coordinates it cannot scale to 64-bit integers.
    let huge = square(0.0, 0.0, 1e15, 1e15);
    let result = std::panic::catch_unwind(|| {
        CrossSection::from_polygons(&[huge], FillRule::Positive).area()
    });
    let err = result.expect_err("expected a panic");
    let msg = err.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(msg.contains("C++ exception"), "{msg}");
    // The library keeps working afterwards.
    assert_close(CrossSection::square(DVec2::ONE, false).area(), 1.0, EPS);
}
