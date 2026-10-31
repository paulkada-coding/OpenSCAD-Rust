//! Output normalization shared by the expected files (produced by OpenSCAD)
//! and the output of our engine. The rules are documented in
//! `tests/compat/README.md`; keep the two in sync.

use std::path::Path;

/// Prefixes that start a new diagnostic message on stderr.
const MESSAGE_PREFIXES: &[&str] = &[
    "ECHO:",
    "WARNING:",
    "ERROR:",
    "TRACE:",
    "DEPRECATED:",
    "FONT-WARNING:",
    "EXPORT-WARNING:",
    "EXPORT-ERROR:",
    "UI-WARNING:",
    "Can't parse file",
];

/// Lines that are never part of the language output: render statistics,
/// timing, cache info and third-party library noise. Matched with
/// `starts_with` after removing leading whitespace.
const NOISE_PREFIXES: &[&str] = &[
    "Fontconfig ",
    "Geometries in cache:",
    "Geometry cache size in bytes:",
    "CGAL Polyhedrons in cache:",
    "CGAL cache size in bytes:",
    "Total rendering time:",
    "Top level object is",
    "Convex:",
    "Facets:",
    "Vertices:",
    "Volumes:",
    "Halfedges:",
    "Edges:",
    "Halffacets:",
    "Contours:",
    "Rendering Polygon Mesh",
    "Simple:",
    "Bounding box:",
    "Current top level object is",
    "Parsing design",
    "Compiling design",
    "Rendering finished",
    "Normalized tree",
    "Compile and preview finished",
];

/// Normalizes the captured stderr of an OpenSCAD-compatible run.
///
/// * the input is decoded as UTF-8 (lossy) and `\r\n` becomes `\n`;
/// * the absolute path of the case directory is removed (so
///   `Can't parse file 'C:\...\cases\x.scad'!` becomes `... 'x.scad'!`);
/// * noise lines (statistics, timing, Fontconfig) are dropped;
/// * only message lines are kept (lines starting with a prefix from
///   [`MESSAGE_PREFIXES`]) plus their continuation lines (a string echoed
///   with an embedded newline, the second line of the color warning, ...);
///   lines before the first message are dropped;
/// * `*** Excluding N frames ***` in TRACE lines becomes `*** Excluding N frames ***`
///   with a literal `N`, because the number depends on the stack size;
/// * trailing empty lines are removed and the result ends with `\n` unless empty.
pub fn normalize_log(raw: &[u8], case_dir: &Path) -> String {
    let text = String::from_utf8_lossy(raw).replace("\r\n", "\n");
    let text = strip_dir(&text, case_dir);
    let mut out: Vec<String> = Vec::new();
    let mut in_message = false;
    for line in text.split('\n') {
        let trimmed = line.trim_start();
        if NOISE_PREFIXES.iter().any(|p| trimmed.starts_with(p)) {
            in_message = false;
            continue;
        }
        if MESSAGE_PREFIXES.iter().any(|p| line.starts_with(p)) {
            in_message = true;
            out.push(normalize_trace(line));
        } else if in_message {
            out.push(line.to_string());
        }
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    join_lines(&out)
}

/// Normalizes a CSG export.
///
/// * UTF-8 (lossy), `\r\n` becomes `\n`;
/// * the absolute path of the case directory is removed;
/// * `timestamp = <integer>` (import/surface: file modification time) becomes
///   `timestamp = 0`;
/// * trailing empty lines are removed and the result ends with `\n` unless empty.
pub fn normalize_csg(raw: &[u8], case_dir: &Path) -> String {
    let text = String::from_utf8_lossy(raw).replace("\r\n", "\n");
    let text = strip_dir(&text, case_dir);
    let mut out: Vec<String> = text.split('\n').map(normalize_timestamp).collect();
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    join_lines(&out)
}

fn join_lines(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

/// Removes `<case_dir>\` and `<case_dir>/` (both slash styles) from `text`.
fn strip_dir(text: &str, case_dir: &Path) -> String {
    let dir = case_dir.to_string_lossy().to_string();
    let dir = dir.strip_prefix(r"\\?\").unwrap_or(&dir).to_string();
    let dir = dir.trim_end_matches(['\\', '/']).to_string();
    if dir.is_empty() {
        return text.to_string();
    }
    let back = dir.replace('/', "\\");
    let fwd = dir.replace('\\', "/");
    let mut s = text.to_string();
    for d in [&back, &fwd] {
        s = s.replace(&format!("{d}\\"), "").replace(&format!("{d}/"), "");
    }
    s
}

fn normalize_trace(line: &str) -> String {
    const START: &str = "*** Excluding ";
    const END: &str = " frames ***";
    if !line.starts_with("TRACE:") {
        return line.to_string();
    }
    if let Some(a) = line.find(START) {
        let rest = &line[a + START.len()..];
        if let Some(b) = rest.find(END)
            && b > 0
            && rest[..b].chars().all(|c| c.is_ascii_digit())
        {
            return format!("{}{START}N{}", &line[..a], &rest[b..]);
        }
    }
    line.to_string()
}

fn normalize_timestamp(line: &str) -> String {
    const KEY: &str = "timestamp = ";
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(pos) = rest.find(KEY) {
        out.push_str(&rest[..pos + KEY.len()]);
        let after = &rest[pos + KEY.len()..];
        let num_len = after
            .char_indices()
            .take_while(|&(i, c)| c.is_ascii_digit() || (i == 0 && c == '-'))
            .count();
        if num_len > 0 && after[..num_len].chars().any(|c| c.is_ascii_digit()) {
            out.push('0');
            rest = &after[num_len..];
        } else {
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_keeps_messages_and_continuations() {
        let dir = Path::new(r"C:\x\cases");
        let raw = b"noise before\r\nECHO: \"a\r\nb\"\r\nWARNING: w\r\nFor a list\r\nGeometries in cache: 1\r\nTotal rendering time: 0:00:00.000\r\nCan't parse file 'C:\\x\\cases\\f.scad'!\r\n\r\n";
        assert_eq!(
            normalize_log(raw, dir),
            "ECHO: \"a\nb\"\nWARNING: w\nFor a list\nCan't parse file 'f.scad'!\n"
        );
    }

    #[test]
    fn trace_frames_are_normalized() {
        let dir = Path::new("C:/none");
        let raw = b"ERROR: x\nTRACE:   *** Excluding 15373 frames ***\n";
        assert_eq!(
            normalize_log(raw, dir),
            "ERROR: x\nTRACE:   *** Excluding N frames ***\n"
        );
    }

    #[test]
    fn csg_timestamp_and_newlines() {
        let dir = Path::new("C:/none");
        let raw = b"surface(file = \"a.dat\", center = false, invert = false, timestamp = -4644214685);\r\n\r\n";
        assert_eq!(
            normalize_csg(raw, dir),
            "surface(file = \"a.dat\", center = false, invert = false, timestamp = 0);\n"
        );
        assert_eq!(normalize_csg(b"\r\n", dir), "");
    }
}
