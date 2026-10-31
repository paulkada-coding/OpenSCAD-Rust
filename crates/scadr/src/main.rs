//! `scadr`: OpenSCAD-compatible command line.
//!
//! `scadr -o out.csg input.scad` evaluates the program, prints ECHO /
//! WARNING / ERROR messages on stderr in OpenSCAD's format and writes the
//! geometry tree in `.csg` form (or writes messages to `.echo`).
//! `-o out.stl` / `-o out.off` also renders the geometry with the mesher
//! and writes the mesh, printing OpenSCAD's render summary on stderr.
//! Behavior and messages are documented in `docs/openscad-behavior.md` §16.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use geometry::Mesh;
use mesh_io::StlFormat;
use mesher::{RenderOptions, Shape};
use scad_eval::{EvalOptions, Evaluator, Level, Message, default_library_paths};

const USAGE: &str = "uso: scadr [-o output.csg|.echo|.stl|.off|-] [--export-format asciistl|binstl|stl|off|csg|echo] [-L dir]... [-D var=expr]... [--enable=feature]... input.scad";

/// Every output suffix / `--export-format` value OpenSCAD accepts.
const OPENSCAD_FORMATS: &[&str] = &[
    "stl", "asciistl", "binstl", "off", "wrl", "3mf", "csg", "dxf", "svg", "pdf", "png", "echo",
    "ast", "term", "nef3", "nefdbg", "param", "pov", "obj",
];

/// What `-o` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    Csg,
    Echo,
    Stl(StlFormat),
    Off,
}

impl ExportFormat {
    /// Maps an OpenSCAD format name to what we can write; `Err` for the
    /// names OpenSCAD knows but we do not support yet.
    fn from_name(name: &str) -> Result<Self, ()> {
        Ok(match name {
            "csg" => Self::Csg,
            "echo" => Self::Echo,
            // OpenSCAD still defaults to ASCII for "stl"; we write binary.
            "stl" | "binstl" => Self::Stl(StlFormat::Binary),
            "asciistl" => Self::Stl(StlFormat::Ascii),
            "off" => Self::Off,
            _ => return Err(()),
        })
    }

    fn is_geometry(self) -> bool {
        matches!(self, Self::Stl(_) | Self::Off)
    }
}

struct Options {
    input: PathBuf,
    output: Option<PathBuf>,
    export_format: Option<String>,
    library_paths: Vec<PathBuf>,
    defines: Vec<(String, String)>,
    object_function: bool,
    vector_swizzle: bool,
    import_function: bool,
}

fn enable_feature(
    feature: &str,
    object_function: &mut bool,
    vector_swizzle: &mut bool,
    import_function: &mut bool,
) {
    match feature {
        "all" => {
            *object_function = true;
            *vector_swizzle = true;
            *import_function = true;
        }
        "object-function" => *object_function = true,
        "vector-swizzle" => *vector_swizzle = true,
        "import-function" => *import_function = true,
        _ => {}
    }
}

fn parse_define(raw: &str) -> Result<(String, String), String> {
    let (name, expr) = raw
        .split_once('=')
        .ok_or_else(|| format!("-D richiede 'nome=espressione', trovato '{raw}'"))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(format!("-D nome variabile vuoto in '{raw}'"));
    }
    Ok((name.to_string(), expr.trim().to_string()))
}

fn parse_args() -> Result<Options, String> {
    let mut input = None;
    let mut output = None;
    let mut export_format = None;
    let mut library_paths = Vec::new();
    let mut defines = Vec::new();
    let mut object_function = false;
    let mut vector_swizzle = false;
    let mut import_function = false;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy();
        match text.as_ref() {
            "-o" => output = Some(PathBuf::from(args.next().ok_or("-o richiede un file")?)),
            "-L" => {
                library_paths.push(PathBuf::from(
                    args.next().ok_or("-L richiede una cartella")?,
                ));
            }
            "-D" => {
                let raw = args.next().ok_or("-D richiede 'nome=espressione'")?;
                defines.push(parse_define(&raw.to_string_lossy())?);
            }
            "-h" | "--help" => return Err(USAGE.into()),
            "--enable" => {
                let feat = args.next().ok_or("--enable richiede una feature")?;
                enable_feature(
                    &feat.to_string_lossy(),
                    &mut object_function,
                    &mut vector_swizzle,
                    &mut import_function,
                );
            }
            "--export-format" => {
                let fmt = args.next().ok_or("--export-format richiede un formato")?;
                export_format = Some(fmt.to_string_lossy().into_owned());
            }
            // Accepted for command-line compatibility; we always use Manifold.
            "--backend" => {
                args.next().ok_or("--backend richiede un valore")?;
            }
            flag if flag.starts_with("--backend=") => {}
            flag if flag.starts_with("--export-format=") => {
                export_format = Some(flag["--export-format=".len()..].to_string());
            }
            flag if flag.starts_with("--enable=") => {
                let feat = &flag["--enable=".len()..];
                enable_feature(
                    feat,
                    &mut object_function,
                    &mut vector_swizzle,
                    &mut import_function,
                );
            }
            flag if flag.starts_with("--libdir=") => {
                library_paths.push(PathBuf::from(&flag["--libdir=".len()..]));
            }
            flag if flag.starts_with("-L") && flag.len() > 2 => {
                library_paths.push(PathBuf::from(&flag[2..]));
            }
            flag if flag.starts_with("-D") && flag.len() > 2 => {
                defines.push(parse_define(&flag[2..])?);
            }
            // `-o -` writes to stdout.
            flag if flag.starts_with('-') && flag != "-" => {
                return Err(format!("opzione sconosciuta: {flag}"));
            }
            _ => input = Some(PathBuf::from(&arg)),
        }
    }
    for std_dir in default_library_paths() {
        if !library_paths.contains(&std_dir) {
            library_paths.push(std_dir);
        }
    }
    Ok(Options {
        input: input.ok_or(USAGE)?,
        output,
        export_format,
        library_paths,
        defines,
        object_function,
        vector_swizzle,
        import_function,
    })
}

fn is_stdout(path: &Path) -> bool {
    path.as_os_str() == "-"
}

/// Resolves the export format like OpenSCAD: `--export-format` (exact,
/// case-sensitive name) wins over the output suffix (case-insensitive).
/// Errors carry OpenSCAD's exact message.
fn resolve_format(output: &Path, export_format: Option<&str>) -> Result<ExportFormat, String> {
    let name = match export_format {
        Some(name) => {
            if !OPENSCAD_FORMATS.contains(&name) {
                return Err(format!(
                    "Unknown --export-format option '{name}'.  Use -h to list available options."
                ));
            }
            name.to_string()
        }
        None => {
            let suffix = if is_stdout(output) {
                String::new()
            } else {
                output
                    .extension()
                    .map(|e| e.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default()
            };
            if !OPENSCAD_FORMATS.contains(&suffix.as_str()) {
                return Err(format!(
                    "Invalid suffix {suffix}. Either add a valid suffix or specify one using the --export-format option."
                ));
            }
            suffix
        }
    };
    ExportFormat::from_name(&name)
        .map_err(|()| format!("ERROR: export format '{name}' is not supported by scadr yet."))
}

fn main() -> ExitCode {
    let options = match parse_args() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };

    // Output checks happen before reading the input, like OpenSCAD.
    let format = match &options.output {
        Some(output) => match resolve_format(output, options.export_format.as_deref()) {
            Ok(format) => Some(format),
            Err(message) => {
                eprintln!("{message}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    if let Some(output) = &options.output
        && !is_stdout(output)
        && let Some(dir) = output.parent().filter(|d| !d.as_os_str().is_empty())
        && !dir.is_dir()
    {
        eprintln!(
            "\n'{}' is not a directory for output file {} - Skipping\n",
            dir.display(),
            output.display()
        );
        return ExitCode::FAILURE;
    }

    let text = match std::fs::read_to_string(&options.input) {
        Ok(text) => text,
        Err(_) => {
            eprintln!("Can't open input file '{}'!\n", options.input.display());
            return ExitCode::FAILURE;
        }
    };

    let echo_mode = format == Some(ExportFormat::Echo);
    let geometry_mode = format.is_some_and(ExportFormat::is_geometry);
    let mut log_buf = String::new();

    // `$preview` is true for CSG / ECHO export and false for a full render,
    // like OpenSCAD.
    let mut evaluator = Evaluator::new(EvalOptions {
        library_paths: options.library_paths,
        defines: options.defines,
        cache: None,
        preview: !geometry_mode,
        object_function: options.object_function,
        vector_swizzle: options.vector_swizzle,
        import_function: options.import_function,
    });
    let (nodes, error) = evaluator.run(&options.input, &text);
    for message in evaluator.messages() {
        emit_message(&evaluator, message, echo_mode, &mut log_buf);
    }

    if let Some(err) = &error {
        evaluator.take_messages();
        evaluator.report_error(err);
        let reported = evaluator.take_messages();
        for message in &reported {
            emit_message(&evaluator, message, echo_mode, &mut log_buf);
        }
        for line in &err.trace {
            emit_raw(&format!("TRACE: {line}"), echo_mode, &mut log_buf);
        }
        if err.is_parse_error() {
            emit_raw(
                &format!("Can't parse file '{}'!", options.input.display()),
                echo_mode,
                &mut log_buf,
            );
            if echo_mode && let Some(output) = &options.output {
                let _ = write_bytes(output, log_buf.as_bytes());
            }
            return ExitCode::FAILURE;
        }
    }

    let (Some(output), Some(format)) = (&options.output, format) else {
        // Like OpenSCAD, an evaluation error still exits with 0.
        return ExitCode::SUCCESS;
    };
    let result = match format {
        ExportFormat::Csg => write_bytes(output, geometry::to_csg_text(&nodes).as_bytes()),
        ExportFormat::Echo => write_bytes(output, log_buf.as_bytes()),
        ExportFormat::Stl(_) | ExportFormat::Off => return export_geometry(&nodes, output, format),
    };
    if let Err(err) = result {
        eprintln!("ERROR: {err}");
        return ExitCode::FAILURE;
    }
    // Like OpenSCAD, an evaluation error still exits with 0.
    ExitCode::SUCCESS
}

/// Renders the top-level nodes and writes a 3D mesh file, with OpenSCAD's
/// messages: render warnings, `Current top level object is empty.` /
/// `... is not a 3D object.` (exit 1, no file) or the render summary.
fn export_geometry(nodes: &[geometry::csg::Node], output: &Path, format: ExportFormat) -> ExitCode {
    let start = Instant::now();
    let rendered = match mesher::render(nodes, &RenderOptions::default()) {
        Ok(rendered) => rendered,
        Err(err) => {
            eprintln!("ERROR: {err}");
            return ExitCode::FAILURE;
        }
    };
    let elapsed = start.elapsed();
    for warning in &rendered.warnings {
        eprintln!("WARNING: {warning}");
    }
    let mesh = match &rendered.shape {
        Shape::Solid(mesh) if !mesh.is_empty() => mesh,
        Shape::Planar { .. } if !rendered.shape.is_empty() => {
            eprintln!("Current top level object is not a 3D object.");
            return ExitCode::FAILURE;
        }
        _ => {
            eprintln!("Current top level object is empty.");
            return ExitCode::FAILURE;
        }
    };
    eprint!("{}", render_summary(mesh, elapsed));
    let mut bytes = Vec::new();
    let encoded = match format {
        ExportFormat::Stl(stl) => mesh_io::write_stl(mesh, &mut bytes, stl),
        _ => mesh_io::write_off(mesh, &mut bytes),
    };
    if let Err(err) = encoded
        .map_err(|e| e.to_string())
        .and_then(|()| write_bytes(output, &bytes))
    {
        eprintln!("ERROR: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// The summary OpenSCAD prints after rendering a 3D object through the
/// Manifold backend (the cache lines are omitted: we have no cache yet).
fn render_summary(mesh: &Mesh, elapsed: Duration) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "Total rendering time: {}", format_duration(elapsed));
    let _ = writeln!(s, "   Top level object is a 3D object (manifold):");
    let _ = writeln!(s, "   Status:     NoError");
    let _ = writeln!(s, "   Genus:      {}", genus(mesh));
    let _ = writeln!(s, "   {:<10}{:>8}", "Vertices:", mesh.vertices.len());
    let _ = writeln!(s, "   {:<10}{:>8}", "Facets:", mesh.triangle_count());
    s
}

/// `H:MM:SS.mmm`, as in `Total rendering time: 0:00:00.002`.
fn format_duration(d: Duration) -> String {
    let ms = d.as_millis();
    format!(
        "{}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// Manifold's genus: `1 - χ / 2` with χ = V - E + F (two disjoint cubes
/// give -1, as OpenSCAD prints).
fn genus(mesh: &Mesh) -> i64 {
    let mut edges = std::collections::HashSet::new();
    let mut used = vec![false; mesh.vertices.len()];
    for t in &mesh.triangles {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            edges.insert((a.min(b), a.max(b)));
            used[a as usize] = true;
        }
    }
    let v = used.iter().filter(|&&u| u).count() as i64;
    let chi = v - edges.len() as i64 + mesh.triangle_count() as i64;
    1 - chi / 2
}

fn write_bytes(output: &Path, bytes: &[u8]) -> Result<(), String> {
    if is_stdout(output) {
        let mut stdout = std::io::stdout().lock();
        return stdout
            .write_all(bytes)
            .and_then(|()| stdout.flush())
            .map_err(|e| format!("Can't write to stdout: {e}"));
    }
    std::fs::write(output, bytes).map_err(|e| format!("Can't write '{}': {e}", output.display()))
}

fn emit_raw(line: &str, echo_mode: bool, log_buf: &mut String) {
    if echo_mode {
        let _ = writeln!(log_buf, "{line}");
    } else {
        eprintln!("{line}");
    }
}

fn emit_message(evaluator: &Evaluator, message: &Message, echo_mode: bool, log_buf: &mut String) {
    let location = match (message.file, message.line) {
        (Some(file), Some(line)) => evaluator
            .files()
            .get(file as usize)
            .map(|f| {
                let path = f.path.display().to_string().replace('\\', "/");
                format!(" in file {path}, line {line}")
            })
            .unwrap_or_default(),
        _ => String::new(),
    };
    match message.level {
        // Echo output has no location suffix.
        Level::Echo => emit_raw(&format!("ECHO: {}", message.text), echo_mode, log_buf),
        level => {
            emit_raw(
                &format!("{}: {}{location}", level.label(), message.text),
                echo_mode,
                log_buf,
            );
            if let Some(extra) = message.continuation {
                emit_raw(extra, echo_mode, log_buf);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geometry::glam::DVec3;

    fn cube_mesh(offset: f64) -> Mesh {
        let mut vertices = Vec::new();
        for i in 0..8 {
            vertices.push(DVec3::new(
                f64::from(i & 1) + offset,
                f64::from((i >> 1) & 1),
                f64::from((i >> 2) & 1),
            ));
        }
        let quads = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        let triangles = quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .collect();
        Mesh {
            vertices,
            triangles,
        }
    }

    #[test]
    fn formats_resolve_like_openscad() {
        let f = |out: &str, fmt: Option<&str>| resolve_format(Path::new(out), fmt);
        assert_eq!(f("a.stl", None), Ok(ExportFormat::Stl(StlFormat::Binary)));
        assert_eq!(f("A.STL", None), Ok(ExportFormat::Stl(StlFormat::Binary)));
        assert_eq!(
            f("a.off", Some("asciistl")),
            Ok(ExportFormat::Stl(StlFormat::Ascii))
        );
        assert_eq!(f("a.dat", Some("off")), Ok(ExportFormat::Off));
        assert_eq!(f("-", Some("csg")), Ok(ExportFormat::Csg));
        assert_eq!(
            f("a.xyz", None).unwrap_err(),
            "Invalid suffix xyz. Either add a valid suffix or specify one using the --export-format option."
        );
        assert_eq!(
            f("noext", None).unwrap_err(),
            "Invalid suffix . Either add a valid suffix or specify one using the --export-format option."
        );
        assert_eq!(
            f("a.stl", Some("STL")).unwrap_err(),
            "Unknown --export-format option 'STL'.  Use -h to list available options."
        );
        assert!(
            f("a.3mf", None)
                .unwrap_err()
                .starts_with("ERROR: export format '3mf'")
        );
    }

    #[test]
    fn summary_matches_openscad_layout() {
        let mut two = cube_mesh(0.0);
        let other = cube_mesh(3.0);
        let base = two.vertices.len() as u32;
        two.vertices.extend(other.vertices);
        two.triangles
            .extend(other.triangles.iter().map(|t| t.map(|i| i + base)));
        assert_eq!(genus(&cube_mesh(0.0)), 0);
        assert_eq!(
            render_summary(&two, Duration::from_millis(3_723_004)),
            "Total rendering time: 1:02:03.004\n   Top level object is a 3D object (manifold):\n   Status:     NoError\n   Genus:      -1\n   Vertices:       16\n   Facets:         24\n"
        );
    }
}
