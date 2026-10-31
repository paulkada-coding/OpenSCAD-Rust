//! `scadr`: OpenSCAD-compatible command line.
//!
//! `scadr -o out.csg input.scad` evaluates the program, prints ECHO /
//! WARNING / ERROR messages on stderr in OpenSCAD's format and writes the
//! geometry tree in `.csg` form.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use scad_eval::{EvalOptions, Evaluator, Level, Message};

const USAGE: &str = "uso: scadr [-o output.csg] [--enable=feature]... input.scad";

struct Options {
    input: PathBuf,
    output: Option<PathBuf>,
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

fn parse_args() -> Result<Options, String> {
    let mut input = None;
    let mut output = None;
    let mut object_function = false;
    let mut vector_swizzle = false;
    let mut import_function = false;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy();
        match text.as_ref() {
            "-o" => output = Some(PathBuf::from(args.next().ok_or("-o richiede un file")?)),
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
            flag if flag.starts_with('-') => return Err(format!("opzione sconosciuta: {flag}")),
            _ => input = Some(PathBuf::from(&arg)),
        }
    }
    Ok(Options {
        input: input.ok_or(USAGE)?,
        output,
        object_function,
        vector_swizzle,
        import_function,
    })
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

    // `$preview` is true for CSG export, like OpenSCAD.
    let mut evaluator = Evaluator::new(EvalOptions {
        preview: true,
        object_function: options.object_function,
        vector_swizzle: options.vector_swizzle,
        import_function: options.import_function,
        ..EvalOptions::default()
    });
    let (nodes, error) = evaluator.run(&options.input, &text);
    for message in evaluator.messages() {
        print_message(&evaluator, message);
    }

    if let Some(err) = &error {
        evaluator.take_messages();
        evaluator.report_error(err);
        let reported = evaluator.take_messages();
        for message in &reported {
            print_message(&evaluator, message);
        }
        for line in &err.trace {
            eprintln!("TRACE: {line}");
        }
        if err.is_parse_error() {
            eprintln!("Can't parse file '{}'!", options.input.display());
            return ExitCode::FAILURE;
        }
    }

    if let Some(output) = &options.output
        && let Err(err) = write_output(output, &nodes)
    {
        eprintln!("ERROR: {err}");
        return ExitCode::FAILURE;
    }
    // Like OpenSCAD, an evaluation error still exits with 0.
    ExitCode::SUCCESS
}

fn write_output(path: &Path, nodes: &[geometry::csg::Node]) -> Result<(), String> {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "csg" => std::fs::write(path, geometry::to_csg_text(nodes))
            .map_err(|e| format!("Can't write '{}': {e}", path.display())),
        other => Err(format!("formato di export non supportato: '.{other}'")),
    }
}

fn print_message(evaluator: &Evaluator, message: &Message) {
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
        Level::Echo => eprintln!("ECHO: {}", message.text),
        level => {
            eprintln!("{}: {}{location}", level.label(), message.text);
            if let Some(extra) = message.continuation {
                eprintln!("{extra}");
            }
        }
    }
}
