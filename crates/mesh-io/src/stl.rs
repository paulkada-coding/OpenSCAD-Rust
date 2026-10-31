//! STL reader (binary and ASCII, auto-detected) and writer.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use geometry::Mesh;
use geometry::glam::DVec3;

use crate::number::format_shortest;

const HEADER_LEN: usize = 80;
const BINARY_PREFIX_LEN: usize = HEADER_LEN + 4;
const BINARY_TRIANGLE_LEN: usize = 50;

#[derive(Debug, thiserror::Error)]
pub enum StlError {
    #[error("errore di I/O: {0}")]
    Io(#[from] io::Error),
    #[error("STL binario troncato: attesi {expected} byte, trovati {found}")]
    Truncated { expected: usize, found: usize },
    #[error("STL ASCII non valido alla riga {line}: {message}")]
    InvalidAscii { line: usize, message: String },
    #[error("il file non contiene triangoli")]
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StlFormat {
    #[default]
    Binary,
    Ascii,
}

pub fn read_stl_file(path: impl AsRef<Path>) -> Result<Mesh, StlError> {
    read_stl(&std::fs::read(path)?)
}

/// Parses an STL file, detecting binary or ASCII encoding.
pub fn read_stl(bytes: &[u8]) -> Result<Mesh, StlError> {
    let mesh = Mesh::from_triangle_soup(read_stl_soup(bytes)?);
    if mesh.is_empty() {
        return Err(StlError::Empty);
    }
    Ok(mesh)
}

/// Parses an STL file (binary or ASCII) into its raw triangles, in file
/// order, without merging vertices or dropping degenerate triangles.
/// An STL without triangles gives an empty list.
pub fn read_stl_soup(bytes: &[u8]) -> Result<Vec<[DVec3; 3]>, StlError> {
    if is_binary(bytes) {
        read_binary(bytes)
    } else {
        read_ascii(bytes)
    }
}

/// Many exporters write binary files whose 80-byte header starts with
/// "solid", so the size check takes precedence over the keyword.
fn is_binary(bytes: &[u8]) -> bool {
    if bytes.len() >= BINARY_PREFIX_LEN {
        let count = binary_triangle_count(bytes);
        if BINARY_PREFIX_LEN + count * BINARY_TRIANGLE_LEN == bytes.len() {
            return true;
        }
    }
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(HEADER_LEN)]);
    !text.trim_start().starts_with("solid")
}

fn binary_triangle_count(bytes: &[u8]) -> usize {
    let raw = &bytes[HEADER_LEN..BINARY_PREFIX_LEN];
    u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize
}

fn read_binary(bytes: &[u8]) -> Result<Vec<[DVec3; 3]>, StlError> {
    if bytes.len() < BINARY_PREFIX_LEN {
        return Err(StlError::Truncated {
            expected: BINARY_PREFIX_LEN,
            found: bytes.len(),
        });
    }
    let count = binary_triangle_count(bytes);
    let expected = BINARY_PREFIX_LEN + count * BINARY_TRIANGLE_LEN;
    if bytes.len() < expected {
        return Err(StlError::Truncated {
            expected,
            found: bytes.len(),
        });
    }

    let read_f32 =
        |at: usize| f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let read_vec = |at: usize| {
        DVec3::new(
            f64::from(read_f32(at)),
            f64::from(read_f32(at + 4)),
            f64::from(read_f32(at + 8)),
        )
    };

    let soup = (0..count).map(|i| {
        // Layout: normal (12 bytes), 3 vertices (36 bytes), attribute (2 bytes).
        let base = BINARY_PREFIX_LEN + i * BINARY_TRIANGLE_LEN + 12;
        [read_vec(base), read_vec(base + 12), read_vec(base + 24)]
    });
    Ok(soup.collect())
}

fn read_ascii(bytes: &[u8]) -> Result<Vec<[DVec3; 3]>, StlError> {
    let text = String::from_utf8_lossy(bytes);
    let mut soup = Vec::new();
    let mut pending: Vec<DVec3> = Vec::with_capacity(3);

    for (line_no, line) in text.lines().enumerate() {
        let mut tokens = line.split_whitespace();
        match tokens.next() {
            Some(word) if word.eq_ignore_ascii_case("vertex") => {
                let mut coord = || -> Result<f64, StlError> {
                    let token = tokens.next().ok_or_else(|| StlError::InvalidAscii {
                        line: line_no + 1,
                        message: "coordinate mancante".into(),
                    })?;
                    token.parse().map_err(|_| StlError::InvalidAscii {
                        line: line_no + 1,
                        message: format!("numero non valido: {token:?}"),
                    })
                };
                pending.push(DVec3::new(coord()?, coord()?, coord()?));
            }
            Some(word) if word.eq_ignore_ascii_case("endloop") => {
                if pending.len() != 3 {
                    return Err(StlError::InvalidAscii {
                        line: line_no + 1,
                        message: format!("attesi 3 vertici, trovati {}", pending.len()),
                    });
                }
                soup.push([pending[0], pending[1], pending[2]]);
                pending.clear();
            }
            _ => {}
        }
    }
    Ok(soup)
}

pub fn write_stl_file(
    mesh: &Mesh,
    path: impl AsRef<Path>,
    format: StlFormat,
) -> Result<(), StlError> {
    let mut out = BufWriter::new(File::create(path)?);
    write_stl(mesh, &mut out, format)?;
    out.flush()?;
    Ok(())
}

pub fn write_stl(mesh: &Mesh, out: &mut impl Write, format: StlFormat) -> io::Result<()> {
    match format {
        StlFormat::Binary => write_binary(mesh, out),
        StlFormat::Ascii => write_ascii(mesh, out),
    }
}

fn write_binary(mesh: &Mesh, out: &mut impl Write) -> io::Result<()> {
    let mut header = [0u8; HEADER_LEN];
    // Same header as OpenSCAD's binary STL export (zero padded).
    let tag = b"OpenSCAD Model\n";
    header[..tag.len()].copy_from_slice(tag);
    out.write_all(&header)?;

    let count = u32::try_from(mesh.triangle_count())
        .map_err(|_| io::Error::other("troppi triangoli per il formato STL"))?;
    out.write_all(&count.to_le_bytes())?;

    let mut record = [0u8; BINARY_TRIANGLE_LEN];
    for i in 0..mesh.triangle_count() {
        let normal = mesh.face_normal(i);
        let [a, b, c] = mesh.triangle(i);
        for (slot, v) in [normal, a, b, c].iter().enumerate() {
            for (axis, value) in v.to_array().iter().enumerate() {
                let at = slot * 12 + axis * 4;
                record[at..at + 4].copy_from_slice(&(*value as f32).to_le_bytes());
            }
        }
        out.write_all(&record)?;
    }
    Ok(())
}

/// ASCII layout and number format of OpenSCAD's ASCII STL export
/// (`solid OpenSCAD_Model`, shortest round-trip numbers, LF line endings).
fn write_ascii(mesh: &Mesh, out: &mut impl Write) -> io::Result<()> {
    let xyz = |v: DVec3| {
        format!(
            "{} {} {}",
            format_shortest(v.x),
            format_shortest(v.y),
            format_shortest(v.z)
        )
    };
    writeln!(out, "solid OpenSCAD_Model")?;
    for i in 0..mesh.triangle_count() {
        writeln!(out, "  facet normal {}", xyz(mesh.face_normal(i)))?;
        writeln!(out, "    outer loop")?;
        for v in mesh.triangle(i) {
            writeln!(out, "      vertex {}", xyz(v))?;
        }
        writeln!(out, "    endloop")?;
        writeln!(out, "  endfacet")?;
    }
    writeln!(out, "endsolid OpenSCAD_Model")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tetrahedron() -> Mesh {
        let p = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(0.0, 10.0, 0.0),
            DVec3::new(0.0, 0.0, 10.0),
        ];
        Mesh {
            vertices: p.to_vec(),
            triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        }
    }

    fn roundtrip(format: StlFormat) -> Mesh {
        let mut bytes = Vec::new();
        write_stl(&tetrahedron(), &mut bytes, format).unwrap();
        read_stl(&bytes).unwrap()
    }

    #[test]
    fn binary_roundtrip() {
        let mesh = roundtrip(StlFormat::Binary);
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.triangle_count(), 4);
        assert!((mesh.volume() - tetrahedron().volume()).abs() < 1e-9);
    }

    #[test]
    fn ascii_roundtrip() {
        let mesh = roundtrip(StlFormat::Ascii);
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.triangle_count(), 4);
        assert!((mesh.volume() - 1000.0 / 6.0).abs() < 1e-9);
    }

    #[test]
    fn binary_with_solid_header_is_detected() {
        let mut bytes = Vec::new();
        write_stl(&tetrahedron(), &mut bytes, StlFormat::Binary).unwrap();
        bytes[..5].copy_from_slice(b"solid");
        assert_eq!(read_stl(&bytes).unwrap().triangle_count(), 4);
    }

    #[test]
    fn truncated_binary_is_rejected() {
        let mut bytes = Vec::new();
        write_stl(&tetrahedron(), &mut bytes, StlFormat::Binary).unwrap();
        bytes.truncate(bytes.len() - 10);
        assert!(matches!(read_stl(&bytes), Err(StlError::Truncated { .. })));
    }

    #[test]
    fn ascii_uses_openscad_layout() {
        let mut bytes = Vec::new();
        write_stl(&tetrahedron(), &mut bytes, StlFormat::Ascii).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "solid OpenSCAD_Model");
        assert_eq!(lines[1], "  facet normal 0 0 -1");
        assert_eq!(lines[2], "    outer loop");
        assert_eq!(lines[3], "      vertex 0 0 0");
        assert_eq!(lines[4], "      vertex 0 10 0");
        assert_eq!(lines.last(), Some(&"endsolid OpenSCAD_Model"));
    }

    #[test]
    fn binary_header_matches_openscad() {
        let mut bytes = Vec::new();
        write_stl(&tetrahedron(), &mut bytes, StlFormat::Binary).unwrap();
        assert!(bytes.starts_with(b"OpenSCAD Model\n\0"));
        assert_eq!(bytes.len(), BINARY_PREFIX_LEN + 4 * BINARY_TRIANGLE_LEN);
    }

    #[test]
    fn soup_keeps_degenerate_triangles() {
        let text = "solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 0 0 0\nvertex 1 0 0\nendloop\nendfacet\nendsolid x\n";
        assert_eq!(read_stl_soup(text.as_bytes()).unwrap().len(), 1);
        assert!(matches!(read_stl(text.as_bytes()), Err(StlError::Empty)));
        assert!(read_stl_soup(b"solid x\nendsolid x\n").unwrap().is_empty());
    }

    #[test]
    fn bad_ascii_reports_line() {
        let text = "solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 nope\n";
        match read_stl(text.as_bytes()) {
            Err(StlError::InvalidAscii { line, .. }) => assert_eq!(line, 4),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
