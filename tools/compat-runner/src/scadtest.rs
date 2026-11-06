//! `--scadtest <LIBRARY>`: turns a library's regression tests in the
//! `openscad-test` format (`<LIBRARY>/tests/*.scadtest`, as BOSL2 has) into
//! one `.scad` case per test, which then run like the text corpus.
//!
//! A `.scadtest` file is TOML with one `[[test]]` table per test; only
//! `name` and `script` are used. Scripts include the library relatively
//! (`include <../std.scad>`); the cases are written elsewhere, so those
//! paths are made absolute.

use std::collections::HashSet;
use std::path::Path;

/// One `[[test]]` table.
#[derive(Debug, PartialEq)]
pub struct Test {
    pub name: String,
    pub script: String,
}

/// Writes `<file>__<test>.scad` for every test of `library` into
/// `cases_dir`, replacing the `.scad` files already there. Returns the
/// number of cases.
pub fn extract(library: &Path, cases_dir: &Path) -> Result<usize, String> {
    let library = std::path::absolute(library)
        .map_err(|e| format!("invalid library path {}: {e}", library.display()))?;
    let tests_dir = library.join("tests");
    let mut files: Vec<_> = std::fs::read_dir(&tests_dir)
        .map_err(|e| format!("cannot read {}: {e}", tests_dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("scadtest"))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("no .scadtest files in {}", tests_dir.display()));
    }

    std::fs::create_dir_all(cases_dir)
        .map_err(|e| format!("cannot create {}: {e}", cases_dir.display()))?;
    for entry in std::fs::read_dir(cases_dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("scad") {
            let _ = std::fs::remove_file(path);
        }
    }

    // `include <../std.scad>` → `include <C:/…/BOSL2/std.scad>`.
    let prefix = format!("<{}/", library.display().to_string().replace('\\', "/"));
    let mut names = HashSet::new();
    let mut count = 0;
    for file in files {
        let text = std::fs::read_to_string(&file)
            .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("tests");
        let stem = stem.strip_prefix("test_").unwrap_or(stem);
        for test in parse(&text).map_err(|e| format!("{}: {e}", file.display()))? {
            let base = format!("{stem}__{}", sanitize(&test.name));
            let mut name = base.clone();
            let mut n = 2;
            while !names.insert(name.clone()) {
                name = format!("{base}_{n}");
                n += 1;
            }
            let script = test.script.replace("<../", &prefix);
            std::fs::write(cases_dir.join(format!("{name}.scad")), script)
                .map_err(|e| format!("cannot write case {name}: {e}"))?;
            count += 1;
        }
    }
    Ok(count)
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The subset of TOML the test files use: `[[test]]` headers, `#`
/// comments, and `key = value` with basic (`"…"`), literal (`'…'`) and
/// multi-line (`'''…'''`, `"""…"""`) strings; other values are skipped.
pub fn parse(text: &str) -> Result<Vec<Test>, String> {
    type Partial = Option<(Option<String>, Option<String>)>;
    fn finish(current: &mut Partial, tests: &mut Vec<Test>) {
        if let Some((Some(name), Some(script))) = current.take() {
            tests.push(Test { name, script });
        }
    }
    let mut tests = Vec::new();
    let mut current: Partial = None;
    let mut lines = text.lines().enumerate();
    while let Some((number, line)) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed == "[[test]]" {
            finish(&mut current, &mut tests);
            current = Some((None, None));
            continue;
        }
        if trimmed.starts_with('[') {
            finish(&mut current, &mut tests);
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            return Err(format!("line {}: expected `key = value`", number + 1));
        };
        let (key, value) = (key.trim(), value.trim_start());
        let parsed = if let Some(rest) = value
            .strip_prefix("'''")
            .map(|r| (r, "'''"))
            .or_else(|| value.strip_prefix("\"\"\"").map(|r| (r, "\"\"\"")))
        {
            let (rest, delimiter) = rest;
            // A newline right after the opening delimiter is not part of it.
            let mut out = String::new();
            let mut first = Some(rest);
            loop {
                let chunk = match first.take() {
                    Some(chunk) => chunk,
                    None => match lines.next() {
                        Some((_, line)) => {
                            out.push('\n');
                            line
                        }
                        None => return Err(format!("line {}: unterminated string", number + 1)),
                    },
                };
                if let Some(end) = chunk.find(delimiter) {
                    out.push_str(&chunk[..end]);
                    break;
                }
                out.push_str(chunk);
            }
            let out = out.strip_prefix('\n').unwrap_or(&out).to_string();
            Some(if delimiter == "\"\"\"" {
                unescape(&out)
            } else {
                out
            })
        } else if let Some(rest) = value.strip_prefix('\'') {
            rest.find('\'').map(|end| rest[..end].to_string())
        } else if let Some(rest) = value.strip_prefix('"') {
            closing_quote(rest).map(|end| unescape(&rest[..end]))
        } else {
            None
        };
        if let (Some((name, script)), Some(value)) = (current.as_mut(), parsed) {
            match key {
                "name" => *name = Some(value),
                "script" => *script = Some(value),
                _ => {}
            }
        }
    }
    finish(&mut current, &mut tests);
    Ok(tests)
}

/// Index of the `"` closing a basic string (escapes skipped).
fn closing_quote(s: &str) -> Option<usize> {
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => return Some(i),
            _ => {}
        }
    }
    None
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tests_with_multiline_scripts() {
        let text = "# comment\n[[test]]\nname = \"test_a\"\nscript = '''\ninclude <../std.scad>\nassert(1 == 1);\n'''\n\n[[test]]\nname = 'test_b'\nexpect_success = false\nscript = '''echo(\"x\");'''\n";
        assert_eq!(
            parse(text).unwrap(),
            [
                Test {
                    name: "test_a".into(),
                    script: "include <../std.scad>\nassert(1 == 1);\n".into(),
                },
                Test {
                    name: "test_b".into(),
                    script: "echo(\"x\");".into(),
                },
            ]
        );
    }

    #[test]
    fn unterminated_strings_are_errors() {
        assert!(parse("[[test]]\nscript = '''\nx").is_err());
    }
}
