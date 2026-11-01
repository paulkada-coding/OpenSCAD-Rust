//! OFF (Object File Format) reader and writer.
//!
//! Reader: `OFF` / `COFF` / `NOFF` / `CNOFF` headers (the counts may follow
//! the keyword on the same line), `#` comments, blank lines, polygonal
//! faces with optional trailing per-face colors (as written by OpenSCAD),
//! extra per-vertex values (normals, colors) ignored. Polygons are
//! triangulated as fans, which is exact for the convex planar faces that
//! OpenSCAD writes.
//!
//! Writer: `OFF`, `<vertices> <faces> 0`, one vertex per line with
//! shortest round-trip numbers, triangles as `3 a b c`. OpenSCAD writes
//! the same layout but with only 6 significant digits; we keep full
//! precision.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use geometry::Mesh;
use geometry::glam::DVec3;

use crate::number::format_shortest;

#[derive(Debug, thiserror::Error)]
pub enum OffError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("invalid OFF at line {line}: {message}")]
    Invalid { line: usize, message: String },
}

/// Vertices and polygonal faces exactly as stored in an OFF file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OffPolygons {
    pub vertices: Vec<DVec3>,
    pub faces: Vec<Vec<u32>>,
}

impl OffPolygons {
    /// Fan triangulation of every face (faces with fewer than 3 vertices
    /// are skipped).
    pub fn to_mesh(&self) -> Mesh {
        let mut triangles = Vec::new();
        for face in &self.faces {
            for k in 1..face.len().saturating_sub(1) {
                triangles.push([face[0], face[k], face[k + 1]]);
            }
        }
        Mesh {
            vertices: self.vertices.clone(),
            triangles,
        }
    }
}

pub fn read_off_file(path: impl AsRef<Path>) -> Result<Mesh, OffError> {
    read_off(&std::fs::read(path)?)
}

/// Parses an OFF file and triangulates its faces.
pub fn read_off(bytes: &[u8]) -> Result<Mesh, OffError> {
    Ok(read_off_polygons(bytes)?.to_mesh())
}

/// Parses an OFF file keeping polygonal faces as they are.
pub fn read_off_polygons(bytes: &[u8]) -> Result<OffPolygons, OffError> {
    let text = String::from_utf8_lossy(bytes);
    // Meaningful lines: comments and blank lines removed, 1-based numbers kept.
    let mut lines = text.lines().enumerate().filter_map(|(i, raw)| {
        let content = raw.split('#').next().unwrap_or("").trim();
        (!content.is_empty()).then_some((i + 1, content))
    });
    let invalid = |line: usize, message: String| OffError::Invalid { line, message };

    let (header_line, header) = lines
        .next()
        .ok_or_else(|| invalid(1, "empty file".into()))?;
    let mut tokens = header.split_whitespace();
    let keyword = tokens.next().unwrap_or_default();
    if !keyword.ends_with("OFF") || keyword.len() > 5 {
        return Err(invalid(
            header_line,
            format!("expected OFF header, found {keyword:?}"),
        ));
    }
    // Counts on the header line or on the next meaningful line.
    let mut count_tokens: Vec<&str> = tokens.collect();
    let mut count_line = header_line;
    if count_tokens.is_empty() {
        let (n, l) = lines
            .next()
            .ok_or_else(|| invalid(header_line + 1, "missing counts".into()))?;
        count_line = n;
        count_tokens = l.split_whitespace().collect();
    }
    let parse_count = |t: Option<&&str>, what: &str| -> Result<usize, OffError> {
        t.and_then(|s| s.parse().ok())
            .ok_or_else(|| invalid(count_line, format!("invalid {what} count")))
    };
    let nv = parse_count(count_tokens.first(), "vertex")?;
    let nf = parse_count(count_tokens.get(1), "face")?;

    let mut out = OffPolygons {
        vertices: Vec::with_capacity(nv),
        faces: Vec::with_capacity(nf),
    };
    for k in 0..nv {
        let (n, l) = lines
            .next()
            .ok_or_else(|| invalid(0, format!("expected {nv} vertices, found {k}")))?;
        let mut coords = l.split_whitespace().map(str::parse::<f64>);
        let mut coord = || -> Result<f64, OffError> {
            match coords.next() {
                Some(Ok(v)) => Ok(v),
                _ => Err(invalid(n, "invalid coordinate".into())),
            }
        };
        out.vertices.push(DVec3::new(coord()?, coord()?, coord()?));
    }
    for k in 0..nf {
        let (n, l) = lines
            .next()
            .ok_or_else(|| invalid(0, format!("expected {nf} faces, found {k}")))?;
        let mut items = l.split_whitespace();
        let count: usize = items
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| invalid(n, "invalid face vertex count".into()))?;
        let mut face = Vec::with_capacity(count);
        for _ in 0..count {
            let index: u32 = items
                .next()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| invalid(n, "invalid vertex index".into()))?;
            if index as usize >= nv {
                return Err(invalid(
                    n,
                    format!("vertex index {index} out of bounds ({nv} vertices)"),
                ));
            }
            face.push(index);
        }
        // Anything left on the line is a face color: ignored.
        out.faces.push(face);
    }
    Ok(out)
}

pub fn write_off_file(mesh: &Mesh, path: impl AsRef<Path>) -> Result<(), OffError> {
    let mut out = BufWriter::new(File::create(path)?);
    write_off(mesh, &mut out)?;
    out.flush()?;
    Ok(())
}

pub fn write_off(mesh: &Mesh, out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "OFF")?;
    writeln!(out, "{} {} 0", mesh.vertices.len(), mesh.triangles.len())?;
    for v in &mesh.vertices {
        writeln!(
            out,
            "{} {} {}",
            format_shortest(v.x),
            format_shortest(v.y),
            format_shortest(v.z)
        )?;
    }
    for [a, b, c] in &mesh.triangles {
        writeln!(out, "3 {a} {b} {c}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tetrahedron() -> Mesh {
        Mesh {
            vertices: vec![
                DVec3::new(0.0, 0.0, 0.0),
                DVec3::new(10.0, 0.0, 0.0),
                DVec3::new(0.0, 10.0, 0.0),
                DVec3::new(0.0, 0.0, 1.0 / 3.0),
            ],
            triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        }
    }

    #[test]
    fn roundtrip_is_exact() {
        let mut bytes = Vec::new();
        write_off(&tetrahedron(), &mut bytes).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.starts_with("OFF\n4 4 0\n0 0 0\n10 0 0\n"));
        assert!(text.ends_with("3 1 2 3\n"));
        assert_eq!(read_off(&bytes).unwrap(), tetrahedron());
    }

    #[test]
    fn reads_openscad_polygons_with_colors() {
        // As written by OpenSCAD: trailing spaces, quads, face colors.
        let text = "OFF\n8 6 0\n0 0 0 \n10 0 0 \n0 10 0 \n10 10 0 \n0 0 10 \n10 0 10 \n0 10 10 \n10 10 10 \n\
                    4 4 5 7 6\n4 2 3 1 0\n4 0 1 5 4\n4 1 3 7 5\n4 3 2 6 7 249 215 44\n4 2 0 4 6\n";
        let polys = read_off_polygons(text.as_bytes()).unwrap();
        assert_eq!(polys.vertices.len(), 8);
        assert_eq!(polys.faces[4], vec![3, 2, 6, 7]);
        let mesh = polys.to_mesh();
        assert_eq!(mesh.triangle_count(), 12);
        assert!((mesh.volume() - 1000.0).abs() < 1e-9);
        assert!((mesh.surface_area() - 600.0).abs() < 1e-9);
    }

    #[test]
    fn header_variants_and_comments() {
        let text =
            "# comment\nOFF 3 1 0 # counts on the header line\n\n0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n";
        let mesh = read_off(text.as_bytes()).unwrap();
        assert_eq!(mesh.triangles, vec![[0, 1, 2]]);
        let coff =
            "COFF\n3 1 0\n0 0 0 255 0 0 255\n1 0 0 255 0 0 255\n0 1 0 255 0 0 255\n3 0 1 2\n";
        assert_eq!(read_off(coff.as_bytes()).unwrap().vertices.len(), 3);
    }

    #[test]
    fn errors_report_lines() {
        match read_off(b"OFF\n3 1 0\n0 0 0\n1 x 0\n0 1 0\n3 0 1 2\n") {
            Err(OffError::Invalid { line, .. }) => assert_eq!(line, 4),
            other => panic!("unexpected: {other:?}"),
        }
        match read_off(b"OFF\n3 1 0\n0 0 0\n1 0 0\n0 1 0\n3 0 1 7\n") {
            Err(OffError::Invalid { line, .. }) => assert_eq!(line, 6),
            other => panic!("unexpected: {other:?}"),
        }
        assert!(read_off(b"PLY\n").is_err());
        assert!(read_off(b"").is_err());
    }
}
