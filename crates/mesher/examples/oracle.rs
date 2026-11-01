//! Compares the mesher with OpenSCAD (development aid, needs OpenSCAD).
//!
//! ```text
//! cargo run -p mesher --example oracle -- cases.txt [filter]
//! cargo run -p mesher --example oracle -- -e "sphere(1, $fn=8);"
//! ```
//!
//! `cases.txt` has one `name: code` per line. For each case the snippet is
//! rendered by both programs (OpenSCAD writes an ASCII STL, whose vertices
//! have full precision) and the vertex sets, volumes and bounds compared.
//! Set `OPENSCAD` to override the executable path.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use geometry::Mesh;
use geometry::glam::DVec3;
use mesher::{RenderOptions, Shape};
use scad_eval::{EvalOptions, Evaluator};

const DEFAULT_OPENSCAD: &str = r"C:\Program Files\OpenSCAD (Nightly)\openscad.exe";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cases: Vec<(String, String)> = match args.first().map(String::as_str) {
        Some("-e") => vec![("inline".into(), args[1..].join(" "))],
        Some(file) => std::fs::read_to_string(file)
            .expect("cannot read cases file")
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with("//"))
            .filter_map(|l| {
                l.split_once(": ")
                    .map(|(a, b)| (a.trim().to_string(), b.to_string()))
            })
            .filter(|(name, _)| args.get(1).is_none_or(|f| name.contains(f.as_str())))
            .collect(),
        None => {
            eprintln!("usage: oracle <cases.txt> [filter] | -e <code>");
            return;
        }
    };
    let dir = PathBuf::from("out/oracle");
    std::fs::create_dir_all(&dir).expect("cannot create out/oracle");
    for (name, code) in cases {
        compare(&dir, &name, &code);
    }
}

fn compare(dir: &Path, name: &str, code: &str) {
    let scad = dir.join(format!("{name}.scad"));
    std::fs::write(&scad, code).expect("write scad");
    // Without the `\\?\` prefix, which OpenSCAD does not understand.
    let scad = std::fs::canonicalize(&scad).expect("canonicalize");
    let scad = PathBuf::from(
        scad.to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string(),
    );

    let mut ev = Evaluator::new(EvalOptions::default());
    let (nodes, err) = ev.run(&scad, code);
    if let Some(e) = err {
        println!("{name}: evaluation error: {e}");
    }
    let files: Vec<PathBuf> = ev.files().iter().map(|f| f.path.clone()).collect();
    let t = Instant::now();
    let out = mesher::render_with_files(&nodes, &RenderOptions::default(), &files);
    let ours_time = t.elapsed().as_secs_f64();
    let (ours, warnings) = match out {
        Ok(out) => {
            let mesh = match out.shape {
                Shape::Solid(m) => Some(m),
                Shape::Planar { fill, .. } => Some(fill),
                Shape::Empty => None,
            };
            (mesh, out.warnings)
        }
        Err(e) => {
            println!("{name}: render error: {e}");
            (None, Vec::new())
        }
    };

    let stl = dir.join(format!("{name}.stl"));
    let _ = std::fs::remove_file(&stl);
    let exe = std::env::var("OPENSCAD").unwrap_or_else(|_| DEFAULT_OPENSCAD.into());
    let t = Instant::now();
    let output = Command::new(exe)
        .args(["--backend=manifold", "-o"])
        .arg(&stl)
        .arg(&scad)
        .output()
        .expect("cannot run OpenSCAD");
    let theirs_time = t.elapsed().as_secs_f64();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let messages: Vec<&str> = stderr
        .lines()
        .filter(|l| l.starts_with("WARNING") || l.starts_with("ERROR"))
        .collect();
    let theirs = std::fs::read_to_string(&stl)
        .ok()
        .map(|s| parse_ascii_stl(&s));

    let line = match (&ours, &theirs) {
        (None, None) => "both empty".to_string(),
        (Some(_), None) => "OpenSCAD empty, ours not".to_string(),
        (None, Some(_)) => "ours empty, OpenSCAD not".to_string(),
        (Some(a), Some(b)) => describe(a, b),
    };
    println!("{name}: {line} | time ours {ours_time:.3}s, openscad {theirs_time:.3}s");
    for m in &messages {
        println!("    oscad> {m}");
    }
    for w in &warnings {
        println!("    ours > WARNING: {w}");
    }
}

fn key(p: DVec3) -> [u64; 3] {
    let p = p + DVec3::ZERO;
    [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]
}

fn describe(ours: &Mesh, theirs: &Mesh) -> String {
    let a: HashSet<[u64; 3]> = ours.vertices.iter().map(|&p| key(p)).collect();
    let b: HashSet<[u64; 3]> = theirs.vertices.iter().map(|&p| key(p)).collect();
    let exact = a.intersection(&b).count();
    // Largest distance from one of our vertices to the nearest of theirs.
    let mut worst: f64 = 0.0;
    let mut worst_at = DVec3::ZERO;
    for p in &ours.vertices {
        if b.contains(&key(*p)) {
            continue;
        }
        let d = theirs
            .vertices
            .iter()
            .map(|q| q.distance(*p))
            .fold(f64::INFINITY, f64::min);
        if d > worst {
            worst = d;
            worst_at = *p;
        }
    }
    let (va, vb) = (ours.volume(), theirs.volume());
    let (ba, bb) = (ours.bounds(), theirs.bounds());
    let bdiff = (ba.min - bb.min)
        .abs()
        .max_element()
        .max((ba.max - bb.max).abs().max_element());
    if std::env::var_os("ORACLE_DUMP").is_some() {
        let mut ours_only: Vec<DVec3> = ours
            .vertices
            .iter()
            .filter(|p| !b.contains(&key(**p)))
            .copied()
            .collect();
        let mut theirs_only: Vec<DVec3> = theirs
            .vertices
            .iter()
            .filter(|p| !a.contains(&key(**p)))
            .copied()
            .collect();
        let by_xyz = |p: &DVec3, q: &DVec3| {
            p.x.total_cmp(&q.x)
                .then(p.y.total_cmp(&q.y))
                .then(p.z.total_cmp(&q.z))
        };
        ours_only.sort_by(by_xyz);
        theirs_only.sort_by(by_xyz);
        for p in ours_only.iter().take(12) {
            println!("    ours only   {:?} {:?} {:?}", p.x, p.y, p.z);
        }
        for p in theirs_only.iter().take(12) {
            println!("    theirs only {:?} {:?} {:?}", p.x, p.y, p.z);
        }
    }
    let verdict = if exact == a.len() && exact == b.len() {
        "EXACT"
    } else if a.len() == b.len() && worst < 1e-9 {
        "ulp"
    } else {
        "DIFF"
    };
    format!(
        "{verdict} verts {}/{} exact {exact}, maxdev {worst:.2e} at {worst_at:?}, vol {va:.6} vs {vb:.6}, bbox diff {bdiff:.2e}, tris {}/{}",
        a.len(),
        b.len(),
        ours.triangles.len(),
        theirs.triangles.len()
    )
}

fn parse_ascii_stl(text: &str) -> Mesh {
    let mut soup = Vec::new();
    let mut pending = Vec::with_capacity(3);
    for line in text.lines() {
        let mut t = line.split_whitespace();
        if t.next() == Some("vertex") {
            let v: Vec<f64> = t.map(|x| x.parse().unwrap_or(f64::NAN)).collect();
            pending.push(DVec3::new(v[0], v[1], v[2]));
            if pending.len() == 3 {
                soup.push([pending[0], pending[1], pending[2]]);
                pending.clear();
            }
        }
    }
    Mesh::from_triangle_soup(soup)
}
