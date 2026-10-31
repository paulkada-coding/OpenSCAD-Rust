//! OFF reader (polygon faces, as written by OpenSCAD and Geomview).

use std::path::Path;

use geometry::glam::DVec3;

#[derive(Debug, thiserror::Error)]
pub enum OffError {
    #[error("errore di I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("file OFF non valido: {0}")]
    Invalid(String),
}

/// Vertices and polygon faces (indices as stored in the file).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OffMesh {
    pub vertices: Vec<DVec3>,
    pub faces: Vec<Vec<usize>>,
}

pub fn read_off_file(path: impl AsRef<Path>) -> Result<OffMesh, OffError> {
    read_off(&String::from_utf8_lossy(&std::fs::read(path)?))
}

/// Parses ASCII OFF. Comments start with `#`; extra per-vertex values
/// (`COFF`, `NOFF`) and per-face colors are ignored.
pub fn read_off(text: &str) -> Result<OffMesh, OffError> {
    let mut lines = text
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty());
    let invalid = |what: &str| OffError::Invalid(what.to_string());
    let first = lines.next().ok_or_else(|| invalid("file vuoto"))?;
    let mut header: Vec<&str> = first.split_whitespace().collect();
    if header.first().is_some_and(|h| h.ends_with("OFF")) {
        header.remove(0);
    }
    if header.is_empty() {
        header = lines
            .next()
            .ok_or_else(|| invalid("intestazione incompleta"))?
            .split_whitespace()
            .collect();
    }
    let nv = parse(
        header
            .first()
            .ok_or_else(|| invalid("numero di vertici mancante"))?,
    )?;
    let nf = parse(
        header
            .get(1)
            .ok_or_else(|| invalid("numero di facce mancante"))?,
    )?;
    let mut vertices = Vec::with_capacity(nv);
    for _ in 0..nv {
        let line = lines.next().ok_or_else(|| invalid("vertici mancanti"))?;
        let v: Vec<f64> = line
            .split_whitespace()
            .take(3)
            .map(parse_f)
            .collect::<Result<_, _>>()?;
        if v.len() < 3 {
            return Err(invalid("vertice incompleto"));
        }
        vertices.push(DVec3::new(v[0], v[1], v[2]));
    }
    let mut faces = Vec::with_capacity(nf);
    for _ in 0..nf {
        let line = lines.next().ok_or_else(|| invalid("facce mancanti"))?;
        let mut tokens = line.split_whitespace();
        let n = parse(tokens.next().ok_or_else(|| invalid("faccia vuota"))?)?;
        let face: Vec<usize> = tokens.take(n).map(parse).collect::<Result<_, _>>()?;
        if face.len() < n {
            return Err(invalid("faccia incompleta"));
        }
        faces.push(face);
    }
    Ok(OffMesh { vertices, faces })
}

fn parse(t: &str) -> Result<usize, OffError> {
    t.parse()
        .map_err(|_| OffError::Invalid(format!("numero intero non valido: {t:?}")))
}

fn parse_f(t: &str) -> Result<f64, OffError> {
    t.parse()
        .map_err(|_| OffError::Invalid(format!("numero non valido: {t:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_openscad_off() {
        let text = "OFF\n4 4 0\n0 0 0 \n1 0 0 \n0 1 0 \n0 0 1 \n3 0 2 1\n3 0 1 3\n3 0 3 2\n3 1 2 3 255 0 0\n";
        let m = read_off(text).unwrap();
        assert_eq!(m.vertices.len(), 4);
        assert_eq!(
            m.faces,
            vec![vec![0, 2, 1], vec![0, 1, 3], vec![0, 3, 2], vec![1, 2, 3]]
        );
        let inline = read_off("OFF 1 0 0\n# comment\n1 2 3\n").unwrap();
        assert_eq!(inline.vertices, vec![DVec3::new(1.0, 2.0, 3.0)]);
    }

    #[test]
    fn rejects_truncated() {
        assert!(read_off("OFF\n4 4 0\n0 0 0\n").is_err());
    }
}
