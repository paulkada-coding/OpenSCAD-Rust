//! Minimal line-based unified diff (LCS), enough for test reports.

use std::fmt::Write as _;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Equal,
    Delete,
    Insert,
}

/// Above this many LCS cells the diff falls back to a plain listing.
const MAX_CELLS: usize = 30_000_000;
const CONTEXT: usize = 3;

/// Returns a unified diff between `expected` and `actual` (`---`/`+++`
/// headers use the given labels). Returns an empty string when equal.
pub fn unified_diff(expected: &str, actual: &str, exp_label: &str, act_label: &str) -> String {
    if expected == actual {
        return String::new();
    }
    let a: Vec<&str> = split_lines(expected);
    let b: Vec<&str> = split_lines(actual);
    let mut out = String::new();
    let _ = writeln!(out, "--- {exp_label}");
    let _ = writeln!(out, "+++ {act_label}");
    let Some(ops) = edit_script(&a, &b) else {
        let _ = writeln!(out, "@@ files too large for a line diff; full listing @@");
        for l in &a {
            let _ = writeln!(out, "-{l}");
        }
        for l in &b {
            let _ = writeln!(out, "+{l}");
        }
        return out;
    };
    if ops.iter().all(|&(op, _, _)| op == Op::Equal) {
        // Only the trailing newline differs.
        let _ = writeln!(out, "@@ difference in trailing newline only @@");
        return out;
    }
    // Group the edit script into hunks with CONTEXT lines around changes.
    let n = ops.len();
    let mut i = 0;
    while i < n {
        if ops[i].0 == Op::Equal {
            i += 1;
            continue;
        }
        let start = i.saturating_sub(CONTEXT);
        let mut end = i;
        // Extend the hunk while changes are separated by <= 2*CONTEXT equal lines.
        loop {
            while end < n && ops[end].0 != Op::Equal {
                end += 1;
            }
            let mut eq = end;
            while eq < n && ops[eq].0 == Op::Equal {
                eq += 1;
            }
            if eq < n && eq - end <= 2 * CONTEXT {
                end = eq;
            } else {
                end = (end + CONTEXT).min(n);
                break;
            }
        }
        let (a_start, b_start) = (ops[start].1, ops[start].2);
        let a_len = ops[start..end].iter().filter(|o| o.0 != Op::Insert).count();
        let b_len = ops[start..end].iter().filter(|o| o.0 != Op::Delete).count();
        let disp = |start: usize, len: usize| if len == 0 { start } else { start + 1 };
        let _ = writeln!(
            out,
            "@@ -{},{} +{},{} @@",
            disp(a_start, a_len),
            a_len,
            disp(b_start, b_len),
            b_len
        );
        for &(op, ai, bi) in &ops[start..end] {
            match op {
                Op::Equal => {
                    let _ = writeln!(out, " {}", a[ai]);
                }
                Op::Delete => {
                    let _ = writeln!(out, "-{}", a[ai]);
                }
                Op::Insert => {
                    let _ = writeln!(out, "+{}", b[bi]);
                }
            }
        }
        i = end;
    }
    out
}

fn split_lines(s: &str) -> Vec<&str> {
    let s = s.strip_suffix('\n').unwrap_or(s);
    if s.is_empty() {
        Vec::new()
    } else {
        s.split('\n').collect()
    }
}

/// Edit script as (op, index in a, index in b); `None` if too large.
fn edit_script(a: &[&str], b: &[&str]) -> Option<Vec<(Op, usize, usize)>> {
    // Strip the common prefix/suffix to keep the table small.
    let mut pre = 0;
    while pre < a.len() && pre < b.len() && a[pre] == b[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < a.len() - pre && suf < b.len() - pre && a[a.len() - 1 - suf] == b[b.len() - 1 - suf]
    {
        suf += 1;
    }
    let am = &a[pre..a.len() - suf];
    let bm = &b[pre..b.len() - suf];
    let (n, m) = (am.len(), bm.len());
    if (n + 1).saturating_mul(m + 1) > MAX_CELLS {
        return None;
    }
    // lcs[i][j] = LCS length of am[i..] and bm[j..]
    let w = m + 1;
    let mut lcs = vec![0u32; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i * w + j] = if am[i] == bm[j] {
                lcs[(i + 1) * w + j + 1] + 1
            } else {
                lcs[(i + 1) * w + j].max(lcs[i * w + j + 1])
            };
        }
    }
    let mut ops = Vec::with_capacity(pre + n + m + suf);
    for k in 0..pre {
        ops.push((Op::Equal, k, k));
    }
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && am[i] == bm[j] {
            ops.push((Op::Equal, pre + i, pre + j));
            i += 1;
            j += 1;
        } else if i < n && (j == m || lcs[(i + 1) * w + j] >= lcs[i * w + j + 1]) {
            ops.push((Op::Delete, pre + i, pre + j));
            i += 1;
        } else {
            ops.push((Op::Insert, pre + i, pre + j));
            j += 1;
        }
    }
    for k in 0..suf {
        ops.push((Op::Equal, pre + n + k, pre + m + k));
    }
    Some(ops)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_is_empty() {
        assert_eq!(unified_diff("a\nb\n", "a\nb\n", "e", "a"), "");
    }

    #[test]
    fn simple_change() {
        let d = unified_diff("a\nb\nc\n", "a\nx\nc\n", "e", "a");
        assert_eq!(d, "--- e\n+++ a\n@@ -1,3 +1,3 @@\n a\n-b\n+x\n c\n");
    }

    #[test]
    fn insert_into_empty() {
        let d = unified_diff("", "x\n", "e", "a");
        assert_eq!(d, "--- e\n+++ a\n@@ -0,0 +1,1 @@\n+x\n");
    }
}
