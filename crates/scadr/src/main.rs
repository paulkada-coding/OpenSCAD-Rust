//! `scadr`: OpenSCAD-compatible command line.
//!
//! `scadr -o out.csg input.scad` evaluates the program, prints ECHO /
//! WARNING / ERROR messages on stderr in OpenSCAD's format and writes the
//! geometry tree in `.csg` form (or writes messages to `.echo`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use scad_eval::{EvalOptions, Evaluator, Level, Message, default_library_paths};

const USAGE: &str = "uso: scadr [-o output.csg|output.echo] [-L dir]... [-D var=expr]... [--enable=feature]... input.scad";

struct Options {
    input: PathBuf,
    output: Option<PathBuf>,
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
            flag if flag.starts_with('-') => return Err(format!("opzione sconosciuta: {flag}")),
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
        library_paths,
        defines,
        object_function,
        vector_swizzle,
        import_function,
    })
}

fn is_echo_output(output: Option<&Path>) -> bool {
    output
        .and_then(Path::extension)
        .is_some_and(|e| e.eq_ignore_ascii_case("echo"))
}

fn main() -> ExitCode {
    let options = match parse_args() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let text = match std::fs::read_to_string(&options.input) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("Can't open input file '{}': {err}", options.input.display());
            return ExitCode::FAILURE;
        }
    };

    let echo_mode = is_echo_output(options.output.as_deref());
    let mut log_buf = String::new();

    // `$preview` is true for CSG / ECHO export, like OpenSCAD.
    let mut evaluator = Evaluator::new(EvalOptions {
        library_paths: options.library_paths,
        defines: options.defines,
        cache: None,
        preview: true,
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
                let _ = std::fs::write(output, &log_buf);
            }
            return ExitCode::FAILURE;
        }
    }

    if let Some(output) = &options.output
        && let Err(err) = write_output(output, &nodes, &log_buf)
    {
        eprintln!("ERROR: {err}");
        return ExitCode::FAILURE;
    }
    // Like OpenSCAD, an evaluation error still exits with 0.
    ExitCode::SUCCESS
}

fn write_output(path: &Path, nodes: &[geometry::csg::Node], log_buf: &str) -> Result<(), String> {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "csg" => std::fs::write(path, geometry::to_csg_text(nodes))
            .map_err(|e| format!("Can't write '{}': {e}", path.display())),
        "echo" => std::fs::write(path, log_buf)
            .map_err(|e| format!("Can't write '{}': {e}", path.display())),
        other => Err(format!("formato di export non supportato: '.{other}'")),
    }
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
