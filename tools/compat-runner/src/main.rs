//! `compat-runner`: runs the OpenSCAD compatibility corpus in `tests/compat`
//! against our engine (`scadr`) and compares its output with the expected
//! output recorded from OpenSCAD.
//!
//! See `tests/compat/README.md` for the corpus layout and normalization rules.
//! With `--geometry` it runs the geometry corpus in `tests/compat/geometry`
//! instead (see the `geometry` module and `tests/compat/geometry/README.md`).

mod diff;
mod geometry;
mod json;
mod metrics;
mod normalize;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEFAULT_OPENSCAD: &str = r"C:\Program Files\OpenSCAD (Nightly)\openscad.exe";
/// Directive in the first lines of a case: extra command-line arguments
/// passed to both OpenSCAD and our engine (e.g. `--enable=object-function`).
const ARGS_DIRECTIVE: &str = "// compat-args:";

const USAGE: &str = "\
compat-runner: compare our OpenSCAD engine with OpenSCAD on tests/compat

USAGE:
    compat-runner [OPTIONS]

OPTIONS:
    --engine <PATH>       engine binary (default: <workspace>/target/debug/scadr[.exe])
    --openscad <PATH>     OpenSCAD binary used by --update-expected
                          (default: C:\\Program Files\\OpenSCAD (Nightly)\\openscad.exe)
    --update-expected     regenerate the expected files by running OpenSCAD
    --filter <SUBSTR>     only run cases whose name contains SUBSTR (repeatable: any match)
    -v, --verbose         print unified diffs (geometry: metric tables) for failing cases
    -j, --jobs <N>        parallel jobs (default: number of CPUs)
    --timeout <SECS>      per-case timeout in seconds (default: 30)
    --cases <DIR>         cases directory (default: <workspace>/tests/compat/cases,
                          with --geometry <workspace>/tests/compat/geometry/cases)
    --expected <DIR>      expected directory (default: <workspace>/tests/compat/expected,
                          with --geometry <workspace>/tests/compat/geometry/expected)
    --out-dir <DIR>       where raw outputs are written (default: <workspace>/target/compat-out)
    -h, --help            print this help

GEOMETRY MODE:
    --geometry            render each case to ASCII STL (-o <name>.stl --export-format asciistl)
                          and compare mesh metrics with expected/<name>.json:
                          EXACT (same vertices and triangles), EQUIVALENT (metrics within
                          tolerance, vertices differ) or FAIL
    --tol-volume <REL>    relative volume tolerance (default: 1e-6)
    --tol-area <REL>      relative surface area tolerance (default: 1e-6)
    --tol-bbox <ABS>      absolute bounding box tolerance (default: 1e-6)
    --ignore-log          do not compare the messages (stderr)
    --strict              count EQUIVALENT as a failure in the exit code

PARAMS MODE:
    --params              export Customizer parameters (-o <name>.param) on
                          tests/compat/params and compare exact .param JSON + .log

Exit code: 0 if every case passes (geometry: EXACT or EQUIVALENT; or every update
succeeds), 1 otherwise, 2 on usage errors.
";

struct Options {
    engine: PathBuf,
    openscad: PathBuf,
    update: bool,
    filters: Vec<String>,
    verbose: bool,
    jobs: usize,
    timeout: Duration,
    cases_dir: PathBuf,
    expected_dir: PathBuf,
    out_dir: PathBuf,
    geometry: bool,
    params: bool,
    tolerances: geometry::Tolerances,
    ignore_log: bool,
    strict: bool,
}

fn workspace_root() -> PathBuf {
    // tools/compat-runner -> workspace root
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn default_options() -> Options {
    let root = workspace_root();
    Options {
        engine: root
            .join("target")
            .join("debug")
            .join(format!("scadr{}", std::env::consts::EXE_SUFFIX)),
        openscad: PathBuf::from(DEFAULT_OPENSCAD),
        update: false,
        filters: Vec::new(),
        verbose: false,
        jobs: std::thread::available_parallelism().map_or(4, |n| n.get()),
        timeout: Duration::from_secs(30),
        cases_dir: root.join("tests").join("compat").join("cases"),
        expected_dir: root.join("tests").join("compat").join("expected"),
        out_dir: root.join("target").join("compat-out"),
        geometry: false,
        params: false,
        tolerances: geometry::Tolerances::default(),
        ignore_log: false,
        strict: false,
    }
}

#[cfg(test)]
fn test_options() -> Options {
    default_options()
}

fn parse_tolerance(text: &str, name: &str) -> Result<f64, String> {
    let v: f64 = text.parse().map_err(|e| format!("invalid {name}: {e}"))?;
    if v >= 0.0 && v.is_finite() {
        Ok(v)
    } else {
        Err(format!("{name} must be a non-negative number"))
    }
}

fn parse_args() -> Result<Options, String> {
    let root = workspace_root();
    let mut o = default_options();
    let (mut cases_dir, mut expected_dir) = (None, None);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .ok_or_else(|| format!("missing value for {name}"))
        };
        match a.as_str() {
            "--geometry" => o.geometry = true,
            "--params" => o.params = true,
            "--ignore-log" => o.ignore_log = true,
            "--strict" => o.strict = true,
            "--tol-volume" => {
                o.tolerances.volume_rel = parse_tolerance(&value(&a)?, "--tol-volume")?;
            }
            "--tol-area" => o.tolerances.area_rel = parse_tolerance(&value(&a)?, "--tol-area")?,
            "--tol-bbox" => o.tolerances.bbox_abs = parse_tolerance(&value(&a)?, "--tol-bbox")?,
            "--engine" => o.engine = PathBuf::from(value("--engine")?),
            "--openscad" => o.openscad = PathBuf::from(value("--openscad")?),
            "--update-expected" => o.update = true,
            "--filter" => o.filters.push(value("--filter")?),
            "-v" | "--verbose" => o.verbose = true,
            "-j" | "--jobs" => {
                o.jobs = value("--jobs")?
                    .parse::<usize>()
                    .map_err(|e| format!("invalid --jobs: {e}"))?
                    .max(1);
            }
            "--timeout" => {
                let secs: f64 = value("--timeout")?
                    .parse()
                    .map_err(|e| format!("invalid --timeout: {e}"))?;
                if !(secs > 0.0 && secs.is_finite()) {
                    return Err("--timeout must be a positive number of seconds".into());
                }
                o.timeout = Duration::from_secs_f64(secs);
            }
            "--cases" => cases_dir = Some(PathBuf::from(value("--cases")?)),
            "--expected" => expected_dir = Some(PathBuf::from(value("--expected")?)),
            "--out-dir" => o.out_dir = PathBuf::from(value("--out-dir")?),
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if o.geometry && o.params {
        return Err("--geometry and --params cannot be used together".into());
    }
    let corpus = if o.geometry {
        root.join("tests").join("compat").join("geometry")
    } else if o.params {
        root.join("tests").join("compat").join("params")
    } else {
        root.join("tests").join("compat")
    };
    o.cases_dir = cases_dir.unwrap_or_else(|| corpus.join("cases"));
    o.expected_dir = expected_dir.unwrap_or_else(|| corpus.join("expected"));
    for p in [&mut o.cases_dir, &mut o.expected_dir, &mut o.out_dir] {
        if let Ok(abs) = std::path::absolute(&*p) {
            *p = abs;
        }
    }
    Ok(o)
}

/// One test case: `cases/<name>.scad`.
struct Case {
    name: String,
    path: PathBuf,
    extra_args: Vec<String>,
}

fn discover_cases(o: &Options) -> Result<Vec<Case>, String> {
    let entries = std::fs::read_dir(&o.cases_dir)
        .map_err(|e| format!("cannot read cases dir {}: {e}", o.cases_dir.display()))?;
    let mut cases = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("scad") {
            continue;
        }
        let Some(name) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        if !o.filters.is_empty() && !o.filters.iter().any(|f| name.contains(f.as_str())) {
            continue;
        }
        let extra_args = std::fs::read_to_string(&path)
            .map(|src| parse_directive(&src))
            .unwrap_or_default();
        cases.push(Case {
            name,
            path,
            extra_args,
        });
    }
    cases.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(cases)
}

fn parse_directive(src: &str) -> Vec<String> {
    src.lines()
        .take(5)
        .find_map(|l| l.trim().strip_prefix(ARGS_DIRECTIVE))
        .map(|rest| rest.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Raw result of running one binary on one case.
struct RawRun {
    stderr: Vec<u8>,
    /// Contents of the exported file, `None` if no file was written.
    output: Option<Vec<u8>>,
    exit_code: Option<i32>,
    timed_out: bool,
}

/// Runs `<exe> -o <out_path> [export args] [case args] <name>.scad` in the
/// case directory, with a timeout.
fn run_binary(
    exe: &Path,
    case: &Case,
    out_path: &Path,
    export_args: &[&str],
    timeout: Duration,
) -> Result<RawRun, String> {
    if let Some(out_dir) = out_path.parent() {
        std::fs::create_dir_all(out_dir)
            .map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;
    }
    let _ = std::fs::remove_file(out_path);
    let case_dir = case.path.parent().unwrap_or(Path::new("."));
    let file_name = case.path.file_name().unwrap_or_default();
    let mut child = Command::new(exe)
        .arg("-o")
        .arg(out_path)
        .args(export_args)
        .args(&case.extra_args)
        .arg(file_name)
        .current_dir(case_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
    let mut stdout = child.stdout.take().ok_or("no stdout pipe")?;
    let mut stderr = child.stderr.take().ok_or("no stderr pipe")?;
    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => {
                timed_out = true;
                let _ = child.kill();
                break child.wait().ok();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(format!("wait failed: {e}")),
        }
    };
    let _ = out_thread.join();
    let stderr = err_thread.join().unwrap_or_default();
    let output = std::fs::read(out_path).ok();
    Ok(RawRun {
        stderr,
        output,
        exit_code: status.and_then(|s| s.code()),
        timed_out,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Pass,
    Fail,
    Error,
    Timeout,
    Skip,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Fail => "FAIL",
            Status::Error => "ERROR",
            Status::Timeout => "TIMEOUT",
            Status::Skip => "SKIP",
        }
    }
}

struct Outcome {
    name: String,
    status: Status,
    detail: String,
    diffs: String,
    elapsed: Duration,
}

fn read_expected(path: &Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).replace("\r\n", "\n"))
}

fn normalize_exported(o: &Options, raw: &[u8], case_dir: &Path) -> String {
    if o.params {
        String::from_utf8_lossy(raw).replace("\r\n", "\n")
    } else {
        normalize::normalize_csg(raw, case_dir)
    }
}

fn check_case(o: &Options, case: &Case) -> Outcome {
    let start = Instant::now();
    let ext = if o.params { "param" } else { "csg" };
    let ext_upper = if o.params { "PARAM" } else { "CSG" };
    let mut outcome = Outcome {
        name: case.name.clone(),
        status: Status::Pass,
        detail: String::new(),
        diffs: String::new(),
        elapsed: Duration::ZERO,
    };
    let exp_log_path = o.expected_dir.join(format!("{}.log", case.name));
    let exp_out_path = o.expected_dir.join(format!("{}.{ext}", case.name));
    let Some(exp_log) = read_expected(&exp_log_path) else {
        outcome.status = Status::Skip;
        outcome.detail = "no expected .log (run --update-expected)".into();
        return outcome;
    };
    let exp_out = read_expected(&exp_out_path);
    let case_dir = case.path.parent().unwrap_or(Path::new("."));
    let out_path = o
        .out_dir
        .join("engine")
        .join(format!("{}.{ext}", case.name));
    let run = match run_binary(&o.engine, case, &out_path, &[], o.timeout) {
        Ok(r) => r,
        Err(e) => {
            outcome.status = Status::Error;
            outcome.detail = e;
            outcome.elapsed = start.elapsed();
            return outcome;
        }
    };
    outcome.elapsed = start.elapsed();
    if run.timed_out {
        outcome.status = Status::Timeout;
        outcome.detail = format!("killed after {:.2}s", o.timeout.as_secs_f64());
        return outcome;
    }
    let act_log = normalize::normalize_log(&run.stderr, case_dir);
    let act_out = run
        .output
        .as_deref()
        .map(|b| normalize_exported(o, b, case_dir));
    // Save the normalized actual output next to the raw output for inspection.
    let norm_dir = o.out_dir.join("engine");
    let _ = std::fs::write(norm_dir.join(format!("{}.log", case.name)), &act_log);
    let mut problems = Vec::new();
    if !o.ignore_log && act_log != exp_log {
        problems.push("log differs".to_string());
        outcome.diffs.push_str(&diff::unified_diff(
            &exp_log,
            &act_log,
            &format!("expected/{}.log", case.name),
            "actual log",
        ));
    }
    match (&exp_out, &act_out) {
        (Some(e), Some(a)) if e != a => {
            problems.push(format!("{ext} differs"));
            outcome.diffs.push_str(&diff::unified_diff(
                e,
                a,
                &format!("expected/{}.{ext}", case.name),
                &format!("actual {ext}"),
            ));
        }
        (Some(_), None) => problems.push(format!("no {ext_upper} written")),
        (None, Some(_)) => problems.push(format!("{ext_upper} written but OpenSCAD writes none")),
        _ => {}
    }
    if !problems.is_empty() {
        outcome.status = Status::Fail;
        if let Some(code) = run.exit_code.filter(|&c| c != 0) {
            problems.push(format!("exit code {code}"));
        }
        outcome.detail = problems.join(", ");
    }
    outcome
}

fn update_case(o: &Options, case: &Case) -> Outcome {
    let start = Instant::now();
    let ext = if o.params { "param" } else { "csg" };
    let mut outcome = Outcome {
        name: case.name.clone(),
        status: Status::Pass,
        detail: String::new(),
        diffs: String::new(),
        elapsed: Duration::ZERO,
    };
    let case_dir = case.path.parent().unwrap_or(Path::new("."));
    let out_path = o
        .out_dir
        .join("openscad")
        .join(format!("{}.{ext}", case.name));
    let run = match run_binary(&o.openscad, case, &out_path, &[], o.timeout) {
        Ok(r) => r,
        Err(e) => {
            outcome.status = Status::Error;
            outcome.detail = e;
            return outcome;
        }
    };
    outcome.elapsed = start.elapsed();
    if run.timed_out {
        outcome.status = Status::Timeout;
        outcome.detail = "OpenSCAD timed out; expected files left unchanged".into();
        return outcome;
    }
    let log = normalize::normalize_log(&run.stderr, case_dir);
    let exported = run
        .output
        .as_deref()
        .map(|b| normalize_exported(o, b, case_dir));
    let log_path = o.expected_dir.join(format!("{}.log", case.name));
    let exp_path = o.expected_dir.join(format!("{}.{ext}", case.name));
    let mut changes = Vec::new();
    match write_if_changed(&log_path, &log) {
        Ok(Some(c)) => changes.push(format!("log {c}")),
        Ok(None) => {}
        Err(e) => {
            outcome.status = Status::Error;
            outcome.detail = e;
            return outcome;
        }
    }
    match &exported {
        Some(content) => match write_if_changed(&exp_path, content) {
            Ok(Some(c)) => changes.push(format!("{ext} {c}")),
            Ok(None) => {}
            Err(e) => {
                outcome.status = Status::Error;
                outcome.detail = e;
                return outcome;
            }
        },
        None => {
            if exp_path.exists() {
                let _ = std::fs::remove_file(&exp_path);
                changes.push(format!("{ext} removed (OpenSCAD wrote none)"));
            }
        }
    }
    outcome.detail = if changes.is_empty() {
        "unchanged".into()
    } else {
        changes.join(", ")
    };
    outcome
}

/// Writes `content` to `path` unless identical; returns what happened.
fn write_if_changed(path: &Path, content: &str) -> Result<Option<&'static str>, String> {
    let old = read_expected(path);
    if old.as_deref() == Some(content) {
        return Ok(None);
    }
    std::fs::write(path, content).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(Some(if old.is_some() { "updated" } else { "new" }))
}

/// Runs `job` on every case with `o.jobs` worker threads; results come
/// back in case order.
fn run_parallel<T: Send + 'static>(
    o: &Arc<Options>,
    cases: Vec<Case>,
    job: fn(&Options, &Case) -> T,
) -> Vec<T> {
    let cases = Arc::new(cases);
    let next = Arc::new(AtomicUsize::new(0));
    let results: Arc<Mutex<Vec<(usize, T)>>> = Arc::new(Mutex::new(Vec::new()));
    let workers = o.jobs.min(cases.len()).max(1);
    let handles: Vec<_> = (0..workers)
        .map(|_| {
            let (o, cases, next, results) = (
                Arc::clone(o),
                Arc::clone(&cases),
                Arc::clone(&next),
                Arc::clone(&results),
            );
            std::thread::spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(case) = cases.get(i) else { break };
                    let outcome = job(&o, case);
                    if let Ok(mut r) = results.lock() {
                        r.push((i, outcome));
                    }
                }
            })
        })
        .collect();
    for h in handles {
        let _ = h.join();
    }
    let mut results = match Arc::try_unwrap(results) {
        Ok(m) => m.into_inner().unwrap_or_default(),
        Err(arc) => arc
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default(),
    };
    results.sort_by_key(|(i, _)| *i);
    results.into_iter().map(|(_, r)| r).collect()
}

fn text_job(o: &Options, case: &Case) -> Outcome {
    if o.update {
        update_case(o, case)
    } else {
        check_case(o, case)
    }
}

fn geometry_job(o: &Options, case: &Case) -> geometry::GeoOutcome {
    if o.update {
        geometry::update_case(o, case)
    } else {
        geometry::check_case(o, case)
    }
}

fn print_report(o: &Options, results: &[Outcome]) -> bool {
    let width = results
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    println!("{:<width$}  {:<7}  {:>7}  DETAIL", "CASE", "RESULT", "TIME");
    for r in results {
        println!(
            "{:<width$}  {:<7}  {:>6.2}s  {}",
            r.name,
            r.status.label(),
            r.elapsed.as_secs_f64(),
            r.detail
        );
    }
    if o.verbose {
        for r in results.iter().filter(|r| !r.diffs.is_empty()) {
            println!("\n=== {} ({}) ===", r.name, r.detail);
            print!("{}", r.diffs);
        }
    }
    let count = |s: Status| results.iter().filter(|r| r.status == s).count();
    let total = results.len();
    let pass = count(Status::Pass);
    let pct = if total == 0 {
        0.0
    } else {
        100.0 * pass as f64 / total as f64
    };
    println!();
    if o.update {
        let changed = results
            .iter()
            .filter(|r| r.status == Status::Pass && r.detail != "unchanged")
            .count();
        println!(
            "Updated expected files with OpenSCAD: {total} cases, {changed} changed, {} unchanged, {} errors, {} timeouts",
            pass - changed,
            count(Status::Error),
            count(Status::Timeout)
        );
    } else {
        println!(
            "Total: {total} cases, {pass} passed, {} failed, {} errors, {} timeouts, {} skipped ({pct:.1}% pass)",
            count(Status::Fail),
            count(Status::Error),
            count(Status::Timeout),
            count(Status::Skip)
        );
    }
    pass == total && total > 0
}

fn main() -> ExitCode {
    let o = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let cases = match discover_cases(&o) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    if cases.is_empty() {
        eprintln!(
            "no cases found in {} (filter: {:?})",
            o.cases_dir.display(),
            o.filters
        );
        return ExitCode::from(1);
    }
    if o.update {
        if !o.openscad.is_file() {
            eprintln!(
                "error: OpenSCAD not found at {} (use --openscad <path>)",
                o.openscad.display()
            );
            return ExitCode::from(2);
        }
        if let Err(e) = std::fs::create_dir_all(&o.expected_dir) {
            eprintln!("error: cannot create {}: {e}", o.expected_dir.display());
            return ExitCode::from(2);
        }
        println!(
            "Regenerating expected output for {} cases with {}",
            cases.len(),
            o.openscad.display()
        );
    } else if !o.engine.is_file() {
        // Engine not built yet: report every case as skipped.
        println!(
            "Engine binary not found: {}\nBuild it (cargo build -p <engine crate>) or pass --engine <path>. All {} cases are reported as SKIP.\n",
            o.engine.display(),
            cases.len()
        );
        if o.geometry {
            println!(
                "Total: {} cases, 0 exact, 0 equivalent, 0 failed, 0 errors, 0 timeouts, {} skipped",
                cases.len(),
                cases.len()
            );
            return ExitCode::from(1);
        }
        let results: Vec<Outcome> = cases
            .iter()
            .map(|c| Outcome {
                name: c.name.clone(),
                status: Status::Skip,
                detail: "engine binary missing".into(),
                diffs: String::new(),
                elapsed: Duration::ZERO,
            })
            .collect();
        print_report(&o, &results);
        return ExitCode::from(1);
    } else {
        println!("Running {} cases with {}", cases.len(), o.engine.display());
    }
    let o = Arc::new(o);
    if o.geometry {
        let results = run_parallel(&o, cases, geometry_job);
        return if geometry::print_report(&o, &results) {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        };
    }
    let results = run_parallel(&o, cases, text_job);
    let ok = if o.update {
        print_report(&o, &results);
        results.iter().all(|r| r.status == Status::Pass)
    } else {
        print_report(&o, &results)
    };
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directive_is_parsed() {
        let src = "// compat-args: --enable=object-function --enable=import-function\necho(1);\n";
        assert_eq!(
            parse_directive(src),
            vec!["--enable=object-function", "--enable=import-function"]
        );
        assert!(parse_directive("echo(1);").is_empty());
    }
}
