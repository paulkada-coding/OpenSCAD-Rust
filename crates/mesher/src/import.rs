//! `import()` (STL, OFF) and `surface()` (heightmap `.dat`).

use std::path::{Path, PathBuf};

use geometry::csg::{Node, NodeKind, SourceRef};
use geometry::glam::{DMat4, DVec3};

use crate::eval::{Evaluator, Geom};
use crate::polyset::PolySet;

/// Resolves `file` against the directory of the source file containing the
/// call; returns the path and its OpenSCAD-style display form.
fn resolve(ev: &Evaluator, source: Option<SourceRef>, file: &str) -> (PathBuf, String) {
    let dir = source
        .and_then(|s| ev.options().files.get(s.file as usize))
        .and_then(|p| p.parent())
        .map(Path::to_path_buf);
    let path = Path::new(file);
    match dir {
        Some(dir) if !path.is_absolute() => {
            let display = format!(
                "{}{}{}",
                dir.display().to_string().replace('\\', "/"),
                std::path::MAIN_SEPARATOR,
                file
            );
            (dir.join(path), display)
        }
        _ => (path.to_path_buf(), file.to_string()),
    }
}

pub fn import(ev: &mut Evaluator, node: &Node) -> Geom {
    let NodeKind::Import { file, .. } = &node.kind else {
        return Geom::Empty;
    };
    let src = node.source;
    let (path, display) = resolve(ev, src, file);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let read = |path: &Path| -> Result<Option<PolySet>, String> {
        match ext.as_str() {
            "stl" => mesh_io::read_stl_file(path)
                .map(|mesh| {
                    Some(PolySet {
                        faces: mesh.triangles.iter().map(|t| t.to_vec()).collect(),
                        vertices: mesh.vertices,
                    })
                })
                .map_err(|e| e.to_string()),
            "off" => mesh_io::read_off_file(path)
                .map(|off| {
                    let n = off.vertices.len();
                    // OFF faces are counter-clockwise seen from outside.
                    let faces = off
                        .faces
                        .into_iter()
                        .filter(|f| f.len() >= 3 && f.iter().all(|&i| i < n))
                        .map(|f| f.into_iter().map(|i| i as u32).collect())
                        .collect();
                    Some(PolySet {
                        vertices: off.vertices,
                        faces,
                    })
                })
                .map_err(|e| e.to_string()),
            _ => Ok(None),
        }
    };
    match ext.as_str() {
        "stl" | "off" => {
            if !path.is_file() {
                ev.warn(format!("Can't open import file '{display}'"), src);
                return Geom::Empty;
            }
            match read(&path) {
                Ok(ps) => ev.poly(ps, src),
                Err(e) => {
                    ev.warn(format!("Can't import file '{display}': {e}"), src);
                    Geom::Empty
                }
            }
        }
        "svg" | "dxf" | "3mf" | "obj" | "amf" | "png" => {
            ev.warn(
                format!("import() of .{ext} files is not supported yet: '{display}'"),
                src,
            );
            Geom::Empty
        }
        _ => {
            ev.error(
                format!("Unsupported file format while trying to import file '\"{file}\"'"),
                src,
            );
            Geom::Empty
        }
    }
}

/// `surface()` from a text heightmap: rows along +Y, columns along +X,
/// each cell split into four triangles around its averaged center, on a
/// base one unit below the lowest value.
pub fn surface(ev: &mut Evaluator, node: &Node) -> Geom {
    let NodeKind::Surface { file, center, .. } = &node.kind else {
        return Geom::Empty;
    };
    let src = node.source;
    let (path, display) = resolve(ev, src, file);
    let Ok(text) = std::fs::read_to_string(&path) else {
        ev.warn(format!("The file '{display}' couldn't be opened."), src);
        return Geom::Empty;
    };
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("png"))
    {
        ev.warn("surface() from PNG images is not supported yet", src);
        return Geom::Empty;
    }
    let rows: Vec<Vec<f64>> = text
        .lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .map(|l| {
            l.split_whitespace()
                .map_while(|t| t.parse::<f64>().ok())
                .collect::<Vec<f64>>()
        })
        .filter(|r| !r.is_empty())
        .collect();
    let ps = heightmap(&rows, *center);
    ev.poly(ps, src)
}

fn heightmap(rows: &[Vec<f64>], center: bool) -> Option<PolySet> {
    let ny = rows.len();
    let nx = rows.iter().map(Vec::len).min().unwrap_or(0);
    if nx < 2 || ny < 2 {
        return None;
    }
    let min = rows
        .iter()
        .flat_map(|r| r[..nx].iter())
        .copied()
        .fold(f64::INFINITY, f64::min);
    let base = min - 1.0;
    let mut ps = PolySet::default();
    let grid = |x: usize, y: usize| (y * nx + x) as u32;
    for (y, row) in rows.iter().enumerate() {
        for (x, &z) in row[..nx].iter().enumerate() {
            ps.vertices.push(DVec3::new(x as f64, y as f64, z));
        }
    }
    // Cell centers.
    let centers = ps.vertices.len() as u32;
    for y in 0..ny - 1 {
        for x in 0..nx - 1 {
            let z = (rows[y][x] + rows[y][x + 1] + rows[y + 1][x] + rows[y + 1][x + 1]) / 4.0;
            ps.vertices
                .push(DVec3::new(x as f64 + 0.5, y as f64 + 0.5, z));
            let c = centers + (y * (nx - 1) + x) as u32;
            let (a, b, d, e) = (
                grid(x, y),
                grid(x + 1, y),
                grid(x + 1, y + 1),
                grid(x, y + 1),
            );
            ps.faces.push(vec![a, b, c]);
            ps.faces.push(vec![b, d, c]);
            ps.faces.push(vec![d, e, c]);
            ps.faces.push(vec![e, a, c]);
        }
    }
    // Base points under the border, walking the border counter-clockwise.
    let mut border: Vec<u32> = Vec::new();
    border.extend((0..nx).map(|x| grid(x, 0)));
    border.extend((1..ny).map(|y| grid(nx - 1, y)));
    border.extend((0..nx - 1).rev().map(|x| grid(x, ny - 1)));
    border.extend((1..ny - 1).rev().map(|y| grid(0, y)));
    let base_start = ps.vertices.len() as u32;
    for &v in &border {
        let p = ps.vertices[v as usize];
        ps.vertices.push(DVec3::new(p.x, p.y, base));
    }
    let n = border.len();
    for i in 0..n {
        let j = (i + 1) % n;
        ps.faces.push(vec![
            border[j],
            border[i],
            base_start + i as u32,
            base_start + j as u32,
        ]);
    }
    ps.faces
        .push((0..n as u32).rev().map(|i| base_start + i).collect());
    if center {
        let shift = DVec3::new(-((nx - 1) as f64) / 2.0, -((ny - 1) as f64) / 2.0, 0.0);
        ps.transform(&DMat4::from_translation(shift));
    }
    Some(ps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heightmap_matches_openscad_layout() {
        let ps = heightmap(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.5]], false).unwrap();
        // 6 grid points, 2 centers, 6 base points (as in OpenSCAD's OFF).
        assert_eq!(ps.vertices.len(), 14);
        assert!(ps.vertices.contains(&DVec3::new(1.5, 0.5, 4.125)));
        assert!(ps.vertices.contains(&DVec3::new(1.0, 1.0, 0.0)));
        let m = ps.to_manifold().unwrap();
        assert!(m.volume() > 0.0);
        let centered = heightmap(&[vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.5]], true).unwrap();
        assert!(centered.vertices.contains(&DVec3::new(-1.0, -0.5, 1.0)));
    }
}
