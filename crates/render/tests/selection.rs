//! Picking and selection highlight, rendered on a real (or software) GPU.
//! Skipped with a note when the machine has no usable adapter.

use geometry::Mesh;
use geometry::glam::DVec3;
use render::{HeadlessGpu, OrbitCamera, Projection, Scene, ViewPreset};

/// Unit cube centered on the origin, 12 outward-facing triangles.
fn cube() -> Mesh {
    let p = |x: f64, y: f64, z: f64| DVec3::new(x, y, z) - DVec3::splat(0.5);
    let quads = [
        [p(0., 0., 0.), p(0., 1., 0.), p(1., 1., 0.), p(1., 0., 0.)], // bottom (-z)
        [p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.)], // top (+z)
        [p(0., 0., 0.), p(1., 0., 0.), p(1., 0., 1.), p(0., 0., 1.)], // front (-y)
        [p(0., 1., 0.), p(0., 1., 1.), p(1., 1., 1.), p(1., 1., 0.)], // back (+y)
        [p(0., 0., 0.), p(0., 0., 1.), p(0., 1., 1.), p(0., 1., 0.)], // left (-x)
        [p(1., 0., 0.), p(1., 1., 0.), p(1., 1., 1.), p(1., 0., 1.)], // right (+x)
    ];
    Mesh::from_triangle_soup(
        quads
            .iter()
            .flat_map(|[a, b, c, d]| [[*a, *b, *c], [*a, *c, *d]]),
    )
}

/// The triangles of `mesh` whose normal is `normal`.
fn faces(mesh: &Mesh, normal: DVec3) -> Mesh {
    Mesh {
        vertices: mesh.vertices.clone(),
        triangles: (0..mesh.triangle_count())
            .filter(|&i| mesh.face_normal(i) == normal)
            .map(|i| mesh.triangles[i])
            .collect(),
    }
}

fn gpu() -> Option<HeadlessGpu> {
    match HeadlessGpu::new() {
        Ok(gpu) => Some(gpu),
        Err(err) => {
            eprintln!("skipped: {err}");
            None
        }
    }
}

const SIZE: u32 = 64;

/// Color of the center pixel (sRGB bytes).
fn center(gpu: &HeadlessGpu, scene: &mut Scene, camera: &OrbitCamera) -> [u8; 3] {
    let image = render::render_image(gpu, scene, camera, SIZE, SIZE).expect("render");
    let i = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
    [image.pixels[i], image.pixels[i + 1], image.pixels[i + 2]]
}

fn is_blue([r, g, b]: [u8; 3]) -> bool {
    b > 150 && b > r + 60 && b > g + 30
}

fn is_yellow([r, g, b]: [u8; 3]) -> bool {
    r > 100 && g > 80 && b + 40 < g
}

fn top_camera() -> OrbitCamera {
    let mut camera = OrbitCamera::default();
    camera.set_view(ViewPreset::Top);
    camera.fit(
        geometry::glam::Vec3::splat(-0.5),
        geometry::glam::Vec3::splat(0.5),
    );
    camera
}

#[test]
fn selection_is_drawn_over_visible_faces_only() {
    let Some(gpu) = gpu() else { return };
    let mesh = cube();
    let mut scene = Scene::new(&gpu.device);
    let mut style = *scene.style();
    style.show_edges = false;
    scene.set_style(style);
    scene.set_mesh(&gpu.device, &mesh);

    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut camera = top_camera();
        camera.projection = projection;

        scene.set_selection(&gpu.device, None);
        let plain = center(&gpu, &mut scene, &camera);
        assert!(is_yellow(plain), "{projection:?}: {plain:?}");

        // The top face is the one in view: it turns blue.
        scene.set_selection(&gpu.device, Some(&faces(&mesh, DVec3::Z)));
        let selected = center(&gpu, &mut scene, &camera);
        assert!(is_blue(selected), "{projection:?}: {selected:?}");

        // The bottom face is hidden behind the top one: nothing changes.
        scene.set_selection(&gpu.device, Some(&faces(&mesh, -DVec3::Z)));
        assert_eq!(center(&gpu, &mut scene, &camera), plain, "{projection:?}");
    }

    // Replacing the model clears the selection.
    let camera = top_camera();
    scene.set_selection(&gpu.device, Some(&faces(&mesh, DVec3::Z)));
    scene.set_mesh(&gpu.device, &mesh);
    assert!(is_yellow(center(&gpu, &mut scene, &camera)));
}

#[test]
fn pick_ray_hits_what_is_under_the_cursor() {
    // No GPU work: the ray depends only on the camera and the bounds, but
    // a scene needs a device.
    let Some(gpu) = gpu() else { return };
    let mesh = cube();
    let mut scene = Scene::new(&gpu.device);
    scene.set_mesh(&gpu.device, &mesh);
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut camera = top_camera();
        camera.projection = projection;
        let size = [800.0, 600.0];
        let hit = |pos: [f32; 2]| {
            let (origin, dir) = scene.pick_ray(&camera, size, pos);
            mesh.raycast(origin.as_dvec3(), dir.as_dvec3())
        };
        let middle = hit([400.0, 300.0]).expect("hit");
        assert_eq!(
            mesh.face_normal(middle.triangle),
            DVec3::Z,
            "{projection:?}"
        );
        // The corner of the view is beside the cube.
        assert!(hit([2.0, 2.0]).is_none(), "{projection:?}");
    }
}
