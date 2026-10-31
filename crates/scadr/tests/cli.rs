//! End-to-end checks of the `scadr` command line: messages and exit codes
//! that do not depend on the mesher (see docs/openscad-behavior.md §16).

use std::path::{Path, PathBuf};
use std::process::Command;

struct Run {
    stderr: String,
    code: Option<i32>,
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scadr-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scadr(dir: &Path, args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_scadr"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    Run {
        stderr: String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n"),
        code: out.status.code(),
    }
}

#[test]
fn empty_top_level_writes_nothing() {
    let dir = temp_dir("empty");
    std::fs::write(dir.join("e.scad"), "echo(\"hi\");\n").unwrap();
    for out in ["e.stl", "e.off"] {
        let run = scadr(&dir, &["-o", out, "e.scad"]);
        assert_eq!(
            run.stderr,
            "ECHO: \"hi\"\nCurrent top level object is empty.\n"
        );
        assert_eq!(run.code, Some(1));
        assert!(!dir.join(out).exists());
    }
}

#[test]
fn output_checks_happen_before_evaluation() {
    let dir = temp_dir("checks");
    std::fs::write(dir.join("e.scad"), "echo(\"hi\");\n").unwrap();
    let run = scadr(&dir, &["-o", "e.xyz", "e.scad"]);
    assert_eq!(
        run.stderr,
        "Invalid suffix xyz. Either add a valid suffix or specify one using the --export-format option.\n"
    );
    assert_eq!(run.code, Some(1));

    let run = scadr(&dir, &["--export-format", "foo", "-o", "e.stl", "e.scad"]);
    assert_eq!(
        run.stderr,
        "Unknown --export-format option 'foo'.  Use -h to list available options.\n"
    );
    assert_eq!(run.code, Some(1));

    let missing = Path::new("nodir").join("e.stl");
    let run = scadr(&dir, &["-o", missing.to_str().unwrap(), "e.scad"]);
    assert_eq!(
        run.stderr,
        format!(
            "\n'nodir' is not a directory for output file {} - Skipping\n\n",
            missing.display()
        )
    );
    assert_eq!(run.code, Some(1));

    let run = scadr(&dir, &["-o", "x.stl", "nosuch.scad"]);
    assert_eq!(run.stderr, "Can't open input file 'nosuch.scad'!\n\n");
    assert_eq!(run.code, Some(1));
}

#[test]
fn parse_error_exits_with_1() {
    let dir = temp_dir("parse");
    std::fs::write(dir.join("p.scad"), "cube(;\n").unwrap();
    let run = scadr(&dir, &["-o", "p.stl", "p.scad"]);
    assert_eq!(
        run.stderr,
        "ERROR: Parser error: syntax error in file p.scad, line 1\nCan't parse file 'p.scad'!\n"
    );
    assert_eq!(run.code, Some(1));
    assert!(!dir.join("p.stl").exists());
}

#[test]
fn preview_is_false_for_geometry_and_true_for_csg() {
    let dir = temp_dir("preview");
    std::fs::write(dir.join("v.scad"), "echo($preview);\n").unwrap();
    assert!(
        scadr(&dir, &["-o", "v.stl", "v.scad"])
            .stderr
            .starts_with("ECHO: false\n")
    );
    let run = scadr(&dir, &["--export-format=csg", "-o", "v.dat", "v.scad"]);
    assert_eq!(run.stderr, "ECHO: true\n");
    assert_eq!(run.code, Some(0));
    assert!(dir.join("v.dat").exists());
}
