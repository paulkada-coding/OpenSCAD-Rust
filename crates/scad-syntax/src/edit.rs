//! Surgical source-text edits backed by the lossless syntax tree.
//!
//! Functions in this module splice replacement literals directly into the
//! original source string using token byte ranges from [`crate::Parse`],
//! leaving all surrounding whitespace, indentation, line endings, and
//! Customizer comments untouched.

use std::fmt;

use rowan::TextRange;

use crate::SyntaxKind::{self, *};
use crate::{SyntaxElement, SyntaxError, SyntaxNode, parse};

/// Error returned when an assignment literal cannot be rewritten safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The input file contains syntax errors, so surgical editing is refused.
    SourceHasSyntaxErrors(Vec<SyntaxError>),
    /// No top-level assignment for `name` exists in the source file.
    AssignmentNotFound { name: String },
    /// The replacement text is not a valid single OpenSCAD expression.
    InvalidReplacementLiteral { name: String, literal: String },
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceHasSyntaxErrors(errors) => {
                write!(
                    f,
                    "cannot edit file containing {} syntax error(s)",
                    errors.len()
                )
            }
            Self::AssignmentNotFound { name } => {
                write!(f, "top-level assignment '{name}' not found")
            }
            Self::InvalidReplacementLiteral { name, literal } => {
                write!(f, "invalid replacement literal for '{name}': {literal:?}")
            }
        }
    }
}

impl std::error::Error for EditError {}

/// Replaces the right-hand side expression of the top-level assignment `name`
/// with `new_literal`, preserving all surrounding comments and formatting.
///
/// When `name` is assigned more than once at top level, the last assignment is
/// updated so the edit matches OpenSCAD's last-assignment-wins evaluation rule.
pub fn replace_assignment_value(
    source: &str,
    name: &str,
    new_literal: &str,
) -> Result<String, EditError> {
    replace_assignment_values(source, &[(name, new_literal)])
}

/// Replaces the right-hand side expressions of multiple top-level assignments
/// in a single pass, preserving all surrounding comments and formatting.
///
/// If `updates` lists the same parameter name more than once, the last entry
/// wins.
pub fn replace_assignment_values(
    source: &str,
    updates: &[(&str, &str)],
) -> Result<String, EditError> {
    let parsed = parse(source);
    if !parsed.errors().is_empty() {
        return Err(EditError::SourceHasSyntaxErrors(parsed.errors().to_vec()));
    }
    if updates.is_empty() {
        return Ok(source.to_string());
    }

    // Deduplicate by variable name while keeping the last requested literal.
    let mut deduped: Vec<(&str, &str)> = Vec::with_capacity(updates.len());
    for &(name, literal) in updates {
        if let Some(slot) = deduped.iter_mut().find(|(existing, _)| *existing == name) {
            slot.1 = literal;
        } else {
            deduped.push((name, literal));
        }
    }

    let root = parsed.syntax();
    let mut edits: Vec<(TextRange, &str, &str)> = Vec::with_capacity(deduped.len());
    for (name, literal) in deduped {
        if !is_valid_replacement_expr(literal) {
            return Err(EditError::InvalidReplacementLiteral {
                name: name.to_string(),
                literal: literal.to_string(),
            });
        }
        let range = find_top_level_assignment_value_range(&root, name).ok_or_else(|| {
            EditError::AssignmentNotFound {
                name: name.to_string(),
            }
        })?;
        edits.push((range, name, literal));
    }

    edits.sort_by_key(|(range, _, _)| range.start());

    let mut out = String::with_capacity(source.len());
    let mut cursor = 0usize;
    for &(range, _, literal) in &edits {
        let start = usize::from(range.start());
        let end = usize::from(range.end());
        debug_assert!(start >= cursor && end >= start);
        out.push_str(&source[cursor..start]);
        out.push_str(literal);
        cursor = end;
    }
    out.push_str(&source[cursor..]);

    if !parse(&out).errors().is_empty() {
        let (_, name, literal) = edits[0];
        return Err(EditError::InvalidReplacementLiteral {
            name: name.to_string(),
            literal: literal.to_string(),
        });
    }

    Ok(out)
}

fn find_top_level_assignment_value_range(
    root: &SyntaxNode,
    target_name: &str,
) -> Option<TextRange> {
    let assignment = root
        .children()
        .filter(|node| node.kind() == ASSIGNMENT)
        .filter(|node| assignment_has_name(node, target_name))
        .last()?;
    let expr = assignment
        .children()
        .find(|child| is_expr_node(child.kind()))?;
    non_trivia_range(&expr)
}

fn assignment_has_name(assignment: &SyntaxNode, target_name: &str) -> bool {
    for elem in assignment.children_with_tokens() {
        if let SyntaxElement::Token(tok) = elem {
            if tok.kind().is_trivia() {
                continue;
            }
            return tok.kind() == IDENT && tok.text() == target_name;
        }
    }
    false
}

fn is_expr_node(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        LITERAL
            | NAME_REF
            | PAREN_EXPR
            | UNARY_EXPR
            | BINARY_EXPR
            | TERNARY_EXPR
            | CALL_EXPR
            | INDEX_EXPR
            | MEMBER_EXPR
            | VECTOR_EXPR
            | RANGE_EXPR
            | LET_EXPR
            | ASSERT_EXPR
            | ECHO_EXPR
            | FUNCTION_LIT
    )
}

fn non_trivia_range(node: &SyntaxNode) -> Option<TextRange> {
    let mut first = None;
    let mut last = None;
    for elem in node.descendants_with_tokens() {
        if let SyntaxElement::Token(tok) = elem
            && !tok.kind().is_trivia()
        {
            if first.is_none() {
                first = Some(tok.text_range().start());
            }
            last = Some(tok.text_range().end());
        }
    }
    Some(TextRange::new(first?, last?))
}

fn is_valid_replacement_expr(literal: &str) -> bool {
    if literal.trim().is_empty() {
        return false;
    }
    let probe_src = format!("__scad_edit_probe = {literal};");
    let probe = parse(&probe_src);
    if !probe.errors().is_empty() {
        return false;
    }
    let root = probe.syntax();
    let mut children = root.children();
    let Some(only_stmt) = children.next() else {
        return false;
    };
    if children.next().is_some() || only_stmt.kind() != ASSIGNMENT {
        return false;
    }
    let has_expr = only_stmt.children().any(|c| is_expr_node(c.kind()));
    let has_semi = only_stmt
        .children_with_tokens()
        .filter_map(SyntaxElement::into_token)
        .any(|t| t.kind() == SEMI);
    has_expr && has_semi
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER_SAMPLE: &str = "\
/* [Pezzo] */
// stampa = scatola e coperchio affiancati sul piatto
PEZZO        = \"stampa\";       // [stampa, scatola, coperchio, assieme, taratura]

/* [Configurazione] */
N_FILE       = 1;              // [1:1:4]
LUNGHEZZA    = 70;             // [30:1:270]
PIEDINO      = true;
SERRAGGIO    = -0.25;          // [-0.25:0.05:0.6]
TARATURA     = [0.2, 0.25, 0.3];

module box(PEZZO = \"interno\") {
    local = 99;
    cube([LUNGHEZZA, 10, 10]);
}
";

    #[test]
    fn replaces_single_string_preserving_comments_and_spacing() {
        let edited = replace_assignment_value(HEADER_SAMPLE, "PEZZO", "\"coperchio\"").unwrap();
        assert!(edited.contains(
            "PEZZO        = \"coperchio\";       // [stampa, scatola, coperchio, assieme, taratura]"
        ));
        // Module parameter with the same name is untouched.
        assert!(edited.contains("module box(PEZZO = \"interno\")"));
        assert!(parse(&edited).errors().is_empty());
    }

    #[test]
    fn replaces_multiple_types_in_one_pass() {
        let edited = replace_assignment_values(
            HEADER_SAMPLE,
            &[
                ("LUNGHEZZA", "125"),
                ("PIEDINO", "false"),
                ("SERRAGGIO", "0.35"),
                ("TARATURA", "[0.1, 0.2, 0.3]"),
            ],
        )
        .unwrap();
        assert!(edited.contains("LUNGHEZZA    = 125;             // [30:1:270]"));
        assert!(edited.contains("PIEDINO      = false;"));
        assert!(edited.contains("SERRAGGIO    = 0.35;          // [-0.25:0.05:0.6]"));
        assert!(edited.contains("TARATURA     = [0.1, 0.2, 0.3];"));
        // Unmodified lines remain byte-for-byte identical.
        assert!(edited.contains("N_FILE       = 1;              // [1:1:4]"));
    }

    #[test]
    fn replaces_positive_with_negative_and_escapes_strings() {
        let src = "x = 10; // [-20:20]\nlabel = \"ROSSO\"; // max 8\n";
        let edited =
            replace_assignment_values(src, &[("x", "-15"), ("label", "\"A \\\"B\\\"\\\\C\"")])
                .unwrap();
        assert_eq!(
            edited,
            "x = -15; // [-20:20]\nlabel = \"A \\\"B\\\"\\\\C\"; // max 8\n"
        );
    }

    #[test]
    fn preserves_crlf_and_inline_block_comments() {
        let src = "$fn = /* steps */ 64 /* smooth */ ; // quality\r\ncube(1);\r\n";
        let edited = replace_assignment_value(src, "$fn", "128").unwrap();
        assert_eq!(
            edited,
            "$fn = /* steps */ 128 /* smooth */ ; // quality\r\ncube(1);\r\n"
        );
    }

    #[test]
    fn targets_last_top_level_assignment_when_reassigned() {
        let src = "a = 1;\nb = 2;\na = 3; // [0:10]\n";
        let edited = replace_assignment_value(src, "a", "7").unwrap();
        assert_eq!(edited, "a = 1;\nb = 2;\na = 7; // [0:10]\n");
    }

    #[test]
    fn duplicate_updates_keep_last_value() {
        let src = "a = 1;\n";
        let edited = replace_assignment_values(src, &[("a", "2"), ("a", "9")]).unwrap();
        assert_eq!(edited, "a = 9;\n");
    }

    #[test]
    fn ignores_inner_scope_assignments_and_reports_not_found() {
        let err = replace_assignment_value(HEADER_SAMPLE, "local", "42").unwrap_err();
        assert_eq!(
            err,
            EditError::AssignmentNotFound {
                name: "local".into()
            }
        );
    }

    #[test]
    fn rejects_invalid_replacement_literals_and_broken_sources() {
        for bad in ["", "   ", "1; cube(2)", "\"unterminated", "10 // trailing"] {
            let err = replace_assignment_value("a = 1;\n", "a", bad).unwrap_err();
            assert!(
                matches!(err, EditError::InvalidReplacementLiteral { .. }),
                "expected InvalidReplacementLiteral for {bad:?}, got {err:?}"
            );
        }

        let broken = "a = ;\n";
        let err = replace_assignment_value(broken, "a", "1").unwrap_err();
        assert!(matches!(err, EditError::SourceHasSyntaxErrors(_)));
    }
}
