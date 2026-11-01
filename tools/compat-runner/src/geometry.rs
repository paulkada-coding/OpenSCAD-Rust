//! Geometry mode (`--geometry`): renders each case of
//! `tests/compat/geometry/cases` to an ASCII STL, computes metrics
//! ([`Metrics`]) and compares them with the expected metrics recorded from
//! OpenSCAD in `tests/compat/geometry/expected/<name>.json`.
//!
//! Classification:
//! * `EXACT`: same vertex set and same triangles (rounded to 1e-6), same
//!   messages and exit code;
//! * `EQUIVALENT`: volume, area, bounding box within tolerance and same
//!   topology (manifold flag, components, genus), but vertices or
//!   triangles differ;
//! * `FAIL`: anything else (wrong metrics, no geometry, different log or
//!   exit code).

use std::fmt::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::json::{self, Json};
use crate::metrics::Metrics;
use crate::{Case, Options, RawRun, diff, normalize, run_binary};

/// Extra arguments after `-o <name>.stl`: ASCII STL keeps full precision
/// (OpenSCAD's OFF has 6 significant digits, its binary STL is f32).
pub const EXPORT_ARGS: &[&str] = &["--export-format", "asciistl"];

/// Bump when the JSON layout changes.
const FORMAT_VERSION: f64 = 1.0;

#[derive(Debug, Clone, Copy)]
pub struct Tolerances {
    /// Relative tolerance on the volume.
    pub volume_rel: f64,
    /// Relative tolerance on the surface area.
    pub area_rel: f64,
    /// Absolute tolerance on each bounding box coordinate.
    pub bbox_abs: f64,
}

impl Default for Tolerances {
    fn default() -> Self {
        Self {
            volume_rel: 1e-6,
            area_rel: 1e-6,
            bbox_abs: 1e-6,
        }
    }
}

/// Everything recorded for one case.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub exit_code: Option<i32>,
    /// `Current top level object is empty.` / `... is not a 3D object.`
    pub top_level: Option<String>,
    /// Normalized stderr (messages only, see `tests/compat/README.md`).
    pub log: String,
    /// `None` when no STL was written.
    pub geometry: Option<Metrics>,
}

impl Record {
    pub fn from_run(run: &RawRun, case_dir: &Path) -> Result<Self, String> {
        let stderr = String::from_utf8_lossy(&run.stderr);
        let top_level = stderr
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with("Current top level object is"))
            .map(str::to_string);
        let geometry = match &run.output {
            Some(bytes) => {
                let soup = mesh_io::read_stl_soup(bytes).map_err(|e| format!("bad STL: {e}"))?;
                Some(Metrics::from_soup(&soup))
            }
            None => None,
        };
        Ok(Record {
            exit_code: run.exit_code,
            top_level,
            log: normalize::normalize_log(&run.stderr, case_dir),
            geometry,
        })
    }

    pub fn to_json(&self) -> Json {
        Json::Obj(vec![
            ("format".into(), Json::Num(FORMAT_VERSION)),
            (
                "exit_code".into(),
                self.exit_code
                    .map_or(Json::Null, |c| Json::Num(f64::from(c))),
            ),
            (
                "top_level".into(),
                self.top_level.clone().map_or(Json::Null, Json::Str),
            ),
            ("log".into(), Json::Str(self.log.clone())),
            (
                "geometry".into(),
                self.geometry.as_ref().map_or(Json::Null, Metrics::to_json),
            ),
        ])
    }

    pub fn from_json(v: &Json) -> Result<Self, String> {
        let geometry = match v.get("geometry") {
            Some(g) if !g.is_null() => Some(Metrics::from_json(g)?),
            _ => None,
        };
        Ok(Record {
            exit_code: v.get("exit_code").and_then(Json::as_f64).map(|c| c as i32),
            top_level: v
                .get("top_level")
                .and_then(Json::as_str)
                .map(str::to_string),
            log: v
                .get("log")
                .and_then(Json::as_str)
                .ok_or("missing 'log'")?
                .to_string(),
            geometry,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Exact,
    Equivalent,
    Fail,
    Error,
    Timeout,
    Skip,
}

impl Verdict {
    fn label(self) -> &'static str {
        match self {
            Verdict::Exact => "EXACT",
            Verdict::Equivalent => "EQUIVALENT",
            Verdict::Fail => "FAIL",
            Verdict::Error => "ERROR",
            Verdict::Timeout => "TIMEOUT",
            Verdict::Skip => "SKIP",
        }
    }
}

pub struct GeoOutcome {
    pub name: String,
    pub verdict: Verdict,
    pub detail: String,
    /// Metric table and log diff, printed with `--verbose`.
    pub report: String,
    pub elapsed: Duration,
}

impl GeoOutcome {
    fn new(name: &str) -> Self {
        GeoOutcome {
            name: name.to_string(),
            verdict: Verdict::Exact,
            detail: String::new(),
            report: String::new(),
            elapsed: Duration::ZERO,
        }
    }
}

fn stl_path(dir: &Path, case: &Case) -> std::path::PathBuf {
    dir.join(format!("{}.stl", case.name))
}

/// Runs a binary on a case and turns its output into a [`Record`].
fn record_case(
    exe: &Path,
    o: &Options,
    case: &Case,
    dir: &Path,
    outcome: &mut GeoOutcome,
) -> Option<Record> {
    let start = Instant::now();
    let run = run_binary(exe, case, &stl_path(dir, case), EXPORT_ARGS, o.timeout);
    outcome.elapsed = start.elapsed();
    let run = match run {
        Ok(run) => run,
        Err(e) => {
            outcome.verdict = Verdict::Error;
            outcome.detail = e;
            return None;
        }
    };
    if run.timed_out {
        outcome.verdict = Verdict::Timeout;
        outcome.detail = format!("killed after {:.2}s", o.timeout.as_secs_f64());
        return None;
    }
    let case_dir = case.path.parent().unwrap_or(Path::new("."));
    match Record::from_run(&run, case_dir) {
        Ok(r) => Some(r),
        Err(e) => {
            outcome.verdict = Verdict::Error;
            outcome.detail = e;
            None
        }
    }
}

pub fn check_case(o: &Options, case: &Case) -> GeoOutcome {
    let mut outcome = GeoOutcome::new(&case.name);
    let exp_path = o.expected_dir.join(format!("{}.json", case.name));
    let expected = match std::fs::read_to_string(&exp_path) {
        Err(_) => {
            outcome.verdict = Verdict::Skip;
            outcome.detail = "no expected .json (run --geometry --update-expected)".into();
            return outcome;
        }
        Ok(text) => match json::parse(&text).and_then(|j| Record::from_json(&j)) {
            Ok(r) => r,
            Err(e) => {
                outcome.verdict = Verdict::Error;
                outcome.detail = format!("{}: {e}", exp_path.display());
                return outcome;
            }
        },
    };
    let dir = o.out_dir.join("geometry").join("engine");
    let Some(actual) = record_case(&o.engine, o, case, &dir, &mut outcome) else {
        return outcome;
    };
    let _ = std::fs::write(
        dir.join(format!("{}.json", case.name)),
        actual.to_json().to_pretty(),
    );
    compare(o, &expected, &actual, &mut outcome);
    outcome
}

/// Compares two records and fills verdict, detail and report.
pub fn compare(o: &Options, expected: &Record, actual: &Record, outcome: &mut GeoOutcome) {
    let mut problems: Vec<String> = Vec::new();
    if expected.exit_code != actual.exit_code {
        problems.push(format!(
            "exit code {} (expected {})",
            code_text(actual.exit_code),
            code_text(expected.exit_code)
        ));
    }
    if !o.ignore_log && expected.log != actual.log {
        problems.push("log differs".into());
        outcome.report.push_str(&diff::unified_diff(
            &expected.log,
            &actual.log,
            &format!("expected/{}.json log", outcome.name),
            "actual log",
        ));
    }
    let mut equivalent_note = None;
    match (&expected.geometry, &actual.geometry) {
        (None, None) => {
            if expected.top_level != actual.top_level {
                problems.push(format!(
                    "{:?} (expected {:?})",
                    actual.top_level.as_deref().unwrap_or("-"),
                    expected.top_level.as_deref().unwrap_or("-")
                ));
            }
        }
        (Some(_), None) => problems.push(format!(
            "no geometry written{}",
            actual
                .top_level
                .as_deref()
                .map(|t| format!(" ({t})"))
                .unwrap_or_default()
        )),
        (None, Some(_)) => problems.push(format!(
            "geometry written, expected none ({})",
            expected.top_level.as_deref().unwrap_or("no file")
        )),
        (Some(e), Some(a)) => {
            let table = MetricTable::build(e, a, &o.tolerances);
            outcome.report.insert_str(0, &table.render());
            problems.extend(table.failures());
            if e.vertex_hash != a.vertex_hash || e.triangle_hash != a.triangle_hash {
                equivalent_note = Some(table.summary_of_differences(e, a));
            }
        }
    }
    let base = expected
        .geometry
        .as_ref()
        .map(|g| format!("{} tri, {} vert", g.triangles, g.vertices));
    if !problems.is_empty() {
        outcome.verdict = Verdict::Fail;
        outcome.detail = problems.join("; ");
    } else if let Some(note) = equivalent_note {
        outcome.verdict = Verdict::Equivalent;
        outcome.detail = note;
    } else {
        outcome.verdict = Verdict::Exact;
        outcome.detail = base.unwrap_or_else(|| {
            expected
                .top_level
                .clone()
                .unwrap_or_else(|| "no geometry".into())
        });
    }
    if outcome.verdict == Verdict::Exact {
        outcome.report.clear();
    }
}

fn code_text(c: Option<i32>) -> String {
    c.map_or_else(|| "none".into(), |c| c.to_string())
}

struct MetricRow {
    name: &'static str,
    expected: String,
    actual: String,
    delta: String,
    /// `None`: informational only (does not decide the verdict).
    ok: Option<bool>,
}

struct MetricTable {
    rows: Vec<MetricRow>,
}

fn rel_diff(e: f64, a: f64) -> f64 {
    let scale = e.abs().max(a.abs());
    if scale == 0.0 {
        0.0
    } else {
        (e - a).abs() / scale
    }
}

fn fmt(v: f64) -> String {
    mesh_io::format_shortest(v)
}

impl MetricTable {
    fn build(e: &Metrics, a: &Metrics, tol: &Tolerances) -> Self {
        let mut rows = Vec::new();
        let count = |name, ev: usize, av: usize| MetricRow {
            name,
            expected: ev.to_string(),
            actual: av.to_string(),
            delta: format!("{:+}", av as i64 - ev as i64),
            ok: None,
        };
        rows.push(count("triangles", e.triangles, a.triangles));
        rows.push(count("vertices", e.vertices, a.vertices));
        for (name, ev, av, t) in [
            ("volume", e.volume, a.volume, tol.volume_rel),
            ("area", e.area, a.area, tol.area_rel),
        ] {
            let r = rel_diff(ev, av);
            rows.push(MetricRow {
                name,
                expected: fmt(ev),
                actual: fmt(av),
                delta: format!("rel {r:.3e} (tol {t:e})"),
                ok: Some(r <= t),
            });
        }
        const AXES: [&str; 6] = [
            "bbox_min.x",
            "bbox_min.y",
            "bbox_min.z",
            "bbox_max.x",
            "bbox_max.y",
            "bbox_max.z",
        ];
        let ebox = [e.bbox_min, e.bbox_max].concat();
        let abox = [a.bbox_min, a.bbox_max].concat();
        for (k, name) in AXES.iter().enumerate() {
            let d = (ebox[k] - abox[k]).abs();
            rows.push(MetricRow {
                name,
                expected: fmt(ebox[k]),
                actual: fmt(abox[k]),
                delta: format!("abs {d:.3e} (tol {:e})", tol.bbox_abs),
                ok: Some(d <= tol.bbox_abs),
            });
        }
        let same = |name, ev: String, av: String| {
            let ok = ev == av;
            MetricRow {
                name,
                expected: ev,
                actual: av,
                delta: String::new(),
                ok: Some(ok),
            }
        };
        rows.push(same(
            "manifold",
            e.manifold.to_string(),
            a.manifold.to_string(),
        ));
        rows.push(same(
            "components",
            e.components.to_string(),
            a.components.to_string(),
        ));
        rows.push(same("genus", e.genus.to_string(), a.genus.to_string()));
        let info = |name, ev: String, av: String| MetricRow {
            name,
            delta: if ev == av { "same" } else { "differs" }.into(),
            expected: ev,
            actual: av,
            ok: None,
        };
        rows.push(info(
            "open_edges",
            e.open_edges.to_string(),
            a.open_edges.to_string(),
        ));
        rows.push(info(
            "nonmanifold_edges",
            e.nonmanifold_edges.to_string(),
            a.nonmanifold_edges.to_string(),
        ));
        rows.push(info(
            "vertex_hash",
            e.vertex_hash.clone(),
            a.vertex_hash.clone(),
        ));
        rows.push(info(
            "triangle_hash",
            e.triangle_hash.clone(),
            a.triangle_hash.clone(),
        ));
        MetricTable { rows }
    }

    fn failures(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|r| r.ok == Some(false))
            .map(|r| format!("{} {} (expected {})", r.name, r.actual, r.expected))
            .collect()
    }

    fn summary_of_differences(&self, e: &Metrics, a: &Metrics) -> String {
        let mut s = String::from(if e.vertex_hash != a.vertex_hash {
            "vertices differ"
        } else {
            "same vertices, triangulation differs"
        });
        let _ = write!(
            s,
            "; tri {} vs {}, vert {} vs {}",
            a.triangles, e.triangles, a.vertices, e.vertices
        );
        let _ = write!(
            s,
            "; vol rel {:.1e}, area rel {:.1e}",
            rel_diff(e.volume, a.volume),
            rel_diff(e.area, a.area)
        );
        if let (Some(el), Some(al)) = (&e.vertex_list, &a.vertex_list) {
            let only_e = el.iter().filter(|p| !al.contains(p)).count();
            let only_a = al.iter().filter(|p| !el.contains(p)).count();
            let _ = write!(s, "; {only_e} vertices only expected, {only_a} only actual");
        }
        s
    }

    fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "  {:<18} {:>22} {:>22}  {:<6} DELTA",
            "METRIC", "EXPECTED", "ACTUAL", "OK"
        );
        for r in &self.rows {
            let ok = match r.ok {
                Some(true) => "ok",
                Some(false) => "FAIL",
                None => "-",
            };
            let _ = writeln!(
                out,
                "  {:<18} {:>22} {:>22}  {:<6} {}",
                r.name, r.expected, r.actual, ok, r.delta
            );
        }
        out
    }
}

pub fn update_case(o: &Options, case: &Case) -> GeoOutcome {
    let mut outcome = GeoOutcome::new(&case.name);
    let dir = o.out_dir.join("geometry").join("openscad");
    let Some(record) = record_case(&o.openscad, o, case, &dir, &mut outcome) else {
        if outcome.verdict == Verdict::Timeout {
            outcome.detail = "OpenSCAD timed out; expected file left unchanged".into();
        }
        return outcome;
    };
    let path = o.expected_dir.join(format!("{}.json", case.name));
    let text = record.to_json().to_pretty();
    let old = std::fs::read_to_string(&path)
        .ok()
        .map(|t| t.replace("\r\n", "\n"));
    if old.as_deref() == Some(text.as_str()) {
        outcome.detail = "unchanged".into();
        return outcome;
    }
    if let Err(e) = std::fs::write(&path, &text) {
        outcome.verdict = Verdict::Error;
        outcome.detail = format!("cannot write {}: {e}", path.display());
        return outcome;
    }
    outcome.detail = format!(
        "{} ({})",
        if old.is_some() { "updated" } else { "new" },
        match &record.geometry {
            Some(g) => format!(
                "{} tri, vol {}, {:.2}s",
                g.triangles,
                fmt(g.volume),
                outcome.elapsed.as_secs_f64()
            ),
            None => record
                .top_level
                .clone()
                .unwrap_or_else(|| "no geometry".into()),
        }
    );
    outcome
}

/// Prints the report; returns true when the run counts as a success.
pub fn print_report(o: &Options, results: &[GeoOutcome]) -> bool {
    let width = results
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    println!(
        "{:<width$}  {:<10}  {:>7}  DETAIL",
        "CASE", "RESULT", "TIME"
    );
    for r in results {
        println!(
            "{:<width$}  {:<10}  {:>6.2}s  {}",
            r.name,
            r.verdict.label(),
            r.elapsed.as_secs_f64(),
            r.detail
        );
    }
    if o.verbose {
        for r in results.iter().filter(|r| !r.report.is_empty()) {
            println!("\n=== {} ({}) ===", r.name, r.verdict.label());
            print!("{}", r.report);
        }
    }
    let count = |v: Verdict| results.iter().filter(|r| r.verdict == v).count();
    let total = results.len();
    println!();
    if o.update {
        let changed = results
            .iter()
            .filter(|r| r.verdict == Verdict::Exact && r.detail != "unchanged")
            .count();
        println!(
            "Updated geometry expected files with OpenSCAD: {total} cases, {changed} changed, {} unchanged, {} errors, {} timeouts",
            count(Verdict::Exact) - changed,
            count(Verdict::Error),
            count(Verdict::Timeout)
        );
        return count(Verdict::Exact) == total;
    }
    let (exact, equiv) = (count(Verdict::Exact), count(Verdict::Equivalent));
    let pct = |n: usize| {
        if total == 0 {
            0.0
        } else {
            100.0 * n as f64 / total as f64
        }
    };
    println!(
        "Total: {total} cases, {exact} exact, {equiv} equivalent, {} failed, {} errors, {} timeouts, {} skipped ({:.1}% exact, {:.1}% exact or equivalent)",
        count(Verdict::Fail),
        count(Verdict::Error),
        count(Verdict::Timeout),
        count(Verdict::Skip),
        pct(exact),
        pct(exact + equiv)
    );
    let good = if o.strict { exact } else { exact + equiv };
    total > 0 && good == total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::tests::cube_soup;
    use geometry::glam::DVec3;

    fn record(soup: &[[DVec3; 3]]) -> Record {
        Record {
            exit_code: Some(0),
            top_level: None,
            log: String::new(),
            geometry: Some(Metrics::from_soup(soup)),
        }
    }

    fn verdict(e: &Record, a: &Record) -> (Verdict, String) {
        let o = crate::test_options();
        let mut out = GeoOutcome::new("t");
        compare(&o, e, a, &mut out);
        (out.verdict, out.detail)
    }

    #[test]
    fn classifies_exact_equivalent_fail() {
        let cube = cube_soup(DVec3::ZERO, 2.0);
        let e = record(&cube);
        assert!(verdict(&e, &record(&cube)).0 == Verdict::Exact);

        // Same solid, split differently: each quad uses the other diagonal.
        let mut other = Vec::new();
        for pair in cube.chunks(2) {
            let ([a, b, c], [_, _, d]) = (pair[0], pair[1]);
            other.push([b, c, d]);
            other.push([b, d, a]);
        }
        let (v, detail) = verdict(&e, &record(&other));
        assert!(v == Verdict::Equivalent, "{detail}");
        assert!(detail.starts_with("same vertices, triangulation differs"));

        let (v, detail) = verdict(&e, &record(&cube_soup(DVec3::ZERO, 2.1)));
        assert!(v == Verdict::Fail);
        assert!(detail.contains("volume"), "{detail}");

        let empty = Record {
            exit_code: Some(1),
            top_level: Some("Current top level object is empty.".into()),
            log: String::new(),
            geometry: None,
        };
        let (v, detail) = verdict(&e, &empty);
        assert!(v == Verdict::Fail);
        assert_eq!(
            detail,
            "exit code 1 (expected 0); no geometry written (Current top level object is empty.)"
        );
        assert!(verdict(&empty, &empty).0 == Verdict::Exact);
    }

    #[test]
    fn record_json_roundtrip() {
        let mut r = record(&cube_soup(DVec3::ZERO, 1.0));
        r.log = "WARNING: x\n".into();
        let back = Record::from_json(&json::parse(&r.to_json().to_pretty()).unwrap()).unwrap();
        assert_eq!(back, r);
    }
}
