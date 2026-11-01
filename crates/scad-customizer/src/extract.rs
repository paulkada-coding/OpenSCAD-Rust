//! Extraction of Customizer parameters from OpenSCAD source files.

use geometry::format_number_csg;
use scad_syntax::SyntaxKind::{self, *};
use scad_syntax::lexer::lex;
use scad_syntax::{
    LineIndex, SyntaxElement, SyntaxNode, parse, parse_number_literal, unescape_string_literal,
};

use crate::{
    DEFAULT_GROUP, ExtractedParams, HIDDEN_GROUP, NumberOption, ParamConstraint, ParamValue,
    ParamWarning, ParamWarningKind, Parameter, StringOption, VectorRange,
};

/// Extracts all visible Customizer parameters and range-adjustment warnings
/// from `source`.
pub fn extract_parameters(source: &str) -> ExtractedParams {
    extract_parameters_with_defines(source, &[])
}

/// Extracts Customizer parameters after appending `-D name=expr` command-line
/// overrides (matching OpenSCAD's `-D` + `-o .param` / `-p -P` pipeline).
pub fn extract_parameters_with_defines(
    source: &str,
    defines: &[(String, String)],
) -> ExtractedParams {
    if defines.is_empty() {
        return extract_from_effective_source(source);
    }
    let mut effective = String::with_capacity(source.len() + defines.len() * 24);
    effective.push_str(source);
    for (name, expr) in defines {
        effective.push('\n');
        effective.push_str(name);
        effective.push_str(" = ");
        effective.push_str(expr);
        effective.push(';');
    }
    extract_from_effective_source(&effective)
}

#[derive(Debug)]
struct LineCommentInfo<'a> {
    start_offset: usize,
    line: usize,
    body: &'a str,
}

#[derive(Debug)]
struct GroupChange {
    offset: usize,
    group: Option<String>,
}

fn extract_from_effective_source(source: &str) -> ExtractedParams {
    let parsed = parse(source);
    let line_index = LineIndex::new(source);
    let lines: Vec<&str> = source
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();

    let (cutoff_offset, group_changes, line_comments) = scan_header_tokens(source, &line_index);

    let root = parsed.syntax();
    let mut raw_assignments = Vec::new();
    collect_top_level_assignments(&root, &mut raw_assignments);

    // Deduplicate by parameter name: order follows the first assignment, while
    // value, position, and comment annotations come from the last assignment.
    let mut deduped: Vec<(String, SyntaxNode)> = Vec::new();
    for node in raw_assignments {
        let Some(name) = assignment_name(&node) else {
            continue;
        };
        if let Some(slot) = deduped.iter_mut().find(|(existing, _)| *existing == name) {
            slot.1 = node;
        } else {
            deduped.push((name, node));
        }
    }

    let mut parameters = Vec::new();
    let mut warnings = Vec::new();

    for (name, node) in deduped {
        let start_offset = usize::from(node.text_range().start());
        let end_offset = usize::from(node.text_range().end());
        if end_offset > cutoff_offset {
            continue;
        }

        let group = group_at_offset(&group_changes, start_offset);
        if group.as_deref() == Some(HIDDEN_GROUP) {
            continue;
        }

        let Some(expr_node) = node.children().find(|n| is_expr_kind(n.kind())) else {
            continue;
        };
        let Some(default) = extract_literal_value(&expr_node) else {
            continue;
        };

        let start_line = line_index.line_col(node.text_range().start()).0 as usize;
        let end_line = line_index.line_col(node.text_range().end()).0 as usize;

        let caption = extract_caption(&lines, start_line);
        let trailing_comment = line_comments
            .iter()
            .find(|c| c.line == end_line && c.start_offset >= end_offset)
            .map(|c| c.body);

        let constraint =
            parse_constraint(&name, &default, trailing_comment, start_line, &mut warnings);

        parameters.push(Parameter {
            name,
            group,
            caption,
            default,
            constraint,
            line: start_line,
        });
    }

    ExtractedParams {
        parameters,
        warnings,
    }
}

fn scan_header_tokens<'a>(
    source: &'a str,
    line_index: &LineIndex,
) -> (usize, Vec<GroupChange>, Vec<LineCommentInfo<'a>>) {
    let tokens = lex(source);
    let mut cutoff_offset = source.len();
    let mut group_changes = Vec::new();
    let mut line_comments = Vec::new();
    let mut offset = 0usize;

    for tok in tokens {
        match tok.kind {
            BLOCK_COMMENT => {
                if !tok.text.contains('\n') {
                    group_changes.push(GroupChange {
                        offset,
                        group: parse_group_comment(tok.text),
                    });
                }
            }
            LINE_COMMENT => {
                let line = line_index.line_col((offset as u32).into()).0 as usize;
                let body = tok
                    .text
                    .strip_prefix("//")
                    .unwrap_or(tok.text)
                    .strip_suffix('\r')
                    .unwrap_or_else(|| tok.text.strip_prefix("//").unwrap_or(tok.text));
                line_comments.push(LineCommentInfo {
                    start_offset: offset,
                    line,
                    body,
                });
            }
            UNTERMINATED_COMMENT => {}
            _ => {
                if let Some(brace_rel) = tok.text.find('{') {
                    cutoff_offset = offset + brace_rel;
                    break;
                }
            }
        }
        offset += tok.text.len();
    }

    (cutoff_offset, group_changes, line_comments)
}

fn parse_group_comment(comment_text: &str) -> Option<String> {
    let inner = comment_text
        .strip_prefix("/*")
        .and_then(|s| s.strip_suffix("*/"))
        .unwrap_or("");
    let open = inner.find('[')?;
    let after_open = &inner[open + 1..];
    let close = after_open.find(']')?;
    let name = after_open[..close].trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn group_at_offset(changes: &[GroupChange], offset: usize) -> Option<String> {
    changes
        .iter()
        .rfind(|c| c.offset < offset)
        .map_or_else(|| Some(DEFAULT_GROUP.to_string()), |c| c.group.clone())
}

fn collect_top_level_assignments(node: &SyntaxNode, out: &mut Vec<SyntaxNode>) {
    for child in node.children() {
        match child.kind() {
            ASSIGNMENT => out.push(child),
            BLOCK => collect_top_level_assignments(&child, out),
            _ => {}
        }
    }
}

fn assignment_name(assignment: &SyntaxNode) -> Option<String> {
    for elem in assignment.children_with_tokens() {
        if let SyntaxElement::Token(tok) = elem {
            if tok.kind().is_trivia() {
                continue;
            }
            return if tok.kind() == IDENT {
                Some(tok.text().to_string())
            } else {
                None
            };
        }
    }
    None
}

fn extract_caption(lines: &[&str], assignment_start_line: usize) -> Option<String> {
    if assignment_start_line < 2 {
        return None;
    }
    let prev = lines.get(assignment_start_line - 2)?.trim_start();
    let rest = prev.strip_prefix("//")?.trim();
    (!rest.is_empty()).then(|| rest.to_string())
}

fn is_expr_kind(kind: SyntaxKind) -> bool {
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
            | ERROR
    )
}

fn is_generator_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        LC_EACH | LC_FOR | LC_FOR_C | LC_IF | LC_LET | LC_PAREN
    )
}

fn extract_literal_value(expr: &SyntaxNode) -> Option<ParamValue> {
    if let Some(num) = extract_signed_number(expr) {
        return Some(ParamValue::Number(num));
    }
    match expr.kind() {
        LITERAL => {
            let tok = expr
                .children_with_tokens()
                .filter_map(SyntaxElement::into_token)
                .find(|t| !t.kind().is_trivia())?;
            match tok.kind() {
                TRUE_KW => Some(ParamValue::Bool(true)),
                FALSE_KW => Some(ParamValue::Bool(false)),
                STRING => Some(ParamValue::String(unescape_string_literal(tok.text()))),
                _ => None,
            }
        }
        VECTOR_EXPR => {
            if expr.children().any(|c| is_generator_kind(c.kind())) {
                return None;
            }
            let elem_nodes: Vec<SyntaxNode> =
                expr.children().filter(|c| is_expr_kind(c.kind())).collect();
            if !(1..=4).contains(&elem_nodes.len()) {
                return None;
            }
            let mut values = Vec::with_capacity(elem_nodes.len());
            for elem in &elem_nodes {
                values.push(extract_signed_number(elem)?);
            }
            Some(ParamValue::Vector(values))
        }
        _ => None,
    }
}

fn extract_signed_number(expr: &SyntaxNode) -> Option<f64> {
    match expr.kind() {
        PAREN_EXPR => {
            let inner = expr.children().find(|c| is_expr_kind(c.kind()))?;
            extract_signed_number(&inner)
        }
        UNARY_EXPR => {
            let op_tok = expr
                .children_with_tokens()
                .filter_map(SyntaxElement::into_token)
                .find(|t| !t.kind().is_trivia())?;
            let sign = match op_tok.kind() {
                PLUS => 1.0,
                MINUS => -1.0,
                _ => return None,
            };
            let inner = expr.children().find(|c| is_expr_kind(c.kind()))?;
            let val = extract_signed_number(&inner)?;
            Some(sign * val)
        }
        LITERAL => {
            let tok = expr
                .children_with_tokens()
                .filter_map(SyntaxElement::into_token)
                .find(|t| !t.kind().is_trivia())?;
            if tok.kind() != NUMBER {
                return None;
            }
            let n = parse_number_literal(tok.text());
            n.is_finite().then_some(n)
        }
        _ => None,
    }
}

// --------------------------------------------------------- constraint parsing

fn parse_constraint(
    param_name: &str,
    default: &ParamValue,
    trailing_comment: Option<&str>,
    line: usize,
    warnings: &mut Vec<ParamWarning>,
) -> ParamConstraint {
    let default_constraint = || match default {
        ParamValue::Bool(_) => ParamConstraint::UnconstrainedBool,
        ParamValue::Number(_) => ParamConstraint::NumberSpinbox { step: 1.0 },
        ParamValue::String(_) => ParamConstraint::StringText { max_length: None },
        ParamValue::Vector(_) => ParamConstraint::Vector { range: None },
    };

    let Some(raw_comment) = trailing_comment else {
        return default_constraint();
    };
    let trimmed = raw_comment.trim();
    if trimmed.is_empty() {
        return default_constraint();
    }

    if let Some((bracket_inner, _)) = first_bracketed(trimmed) {
        let inner = bracket_inner.trim();
        if inner.is_empty() {
            return default_constraint();
        }
        return match default {
            ParamValue::Bool(_) => ParamConstraint::UnconstrainedBool,
            ParamValue::Number(initial) => {
                parse_number_bracket(param_name, *initial, inner, line, warnings)
                    .unwrap_or(ParamConstraint::NumberSpinbox { step: 1.0 })
            }
            ParamValue::String(initial) => parse_string_bracket(initial, inner)
                .unwrap_or(ParamConstraint::StringText { max_length: None }),
            ParamValue::Vector(initial) => {
                let range = parse_vector_bracket(param_name, initial, inner, line, warnings);
                ParamConstraint::Vector { range }
            }
        };
    }

    // Unbracketed trailing comment: `// step` on scalar number or `// maxLength` on string.
    match default {
        ParamValue::Number(_) => {
            if let Some(step) = parse_plain_f64(trimmed) {
                ParamConstraint::NumberSpinbox { step }
            } else {
                ParamConstraint::NumberSpinbox { step: 1.0 }
            }
        }
        ParamValue::String(_) => {
            if let Ok(max_len) = trimmed.parse::<usize>() {
                ParamConstraint::StringText {
                    max_length: Some(max_len),
                }
            } else {
                ParamConstraint::StringText { max_length: None }
            }
        }
        _ => default_constraint(),
    }
}

fn first_bracketed(s: &str) -> Option<(&str, &str)> {
    let open = s.find('[')?;
    let rest = &s[open + 1..];
    let close = rest.find(']')?;
    Some((&rest[..close], &rest[close + 1..]))
}

fn parse_plain_f64(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let val = s.parse::<f64>().ok()?;
    val.is_finite().then_some(val)
}

fn parse_number_bracket(
    param_name: &str,
    initial: f64,
    inner: &str,
    line: usize,
    warnings: &mut Vec<ParamWarning>,
) -> Option<ParamConstraint> {
    if inner.contains(',') {
        let mut options = Vec::new();
        for item in inner.split(',') {
            let opt = parse_number_option(item)?;
            options.push(opt);
        }
        ensure_initial_number_option(&mut options, initial);
        return Some(ParamConstraint::NumberEnum { options });
    }

    let parts: Vec<&str> = inner.split(':').collect();
    match parts.as_slice() {
        [only] => {
            let mut max = parse_plain_f64(only)?;
            let min = 0.0;
            // For `[max]` shorthand, OpenSCAD checks `initial > max` (adjusting
            // `max`), while keeping `min = 0.0` even if `initial < 0`.
            if initial > max {
                warnings.push(ParamWarning {
                    name: param_name.to_string(),
                    value: initial,
                    declared_bound: max,
                    kind: ParamWarningKind::AboveMaximum,
                    line,
                });
                max = initial;
            }
            Some(ParamConstraint::NumberRange {
                min,
                max,
                step: 1.0,
            })
        }
        [a, b] => {
            let first = parse_plain_f64(a)?;
            if let Some(second) = parse_plain_f64(b) {
                let (min, max) =
                    adjust_scalar_bounds(param_name, initial, first, second, line, warnings);
                Some(ParamConstraint::NumberRange {
                    min,
                    max,
                    step: 1.0,
                })
            } else {
                // `[10:Alpha]` — single labeled dropdown option on a number!
                let label = strip_optional_quotes(b.trim());
                if label.is_empty() {
                    return None;
                }
                let mut options = vec![NumberOption {
                    name: label.to_string(),
                    value: first,
                }];
                ensure_initial_number_option(&mut options, initial);
                Some(ParamConstraint::NumberEnum { options })
            }
        }
        [a, s, b] => {
            let raw_min = parse_plain_f64(a)?;
            let step = parse_plain_f64(s)?;
            let raw_max = parse_plain_f64(b)?;
            let (min, max) =
                adjust_scalar_bounds(param_name, initial, raw_min, raw_max, line, warnings);
            Some(ParamConstraint::NumberRange { min, max, step })
        }
        _ => None,
    }
}

fn parse_number_option(item: &str) -> Option<NumberOption> {
    let trimmed = item.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some((val_str, label_str)) = trimmed.split_once(':') {
        let value = parse_plain_f64(val_str)?;
        let label = strip_optional_quotes(label_str.trim());
        Some(NumberOption {
            name: label.to_string(),
            value,
        })
    } else {
        let value = parse_plain_f64(trimmed)?;
        Some(NumberOption {
            name: trimmed.to_string(),
            value,
        })
    }
}

fn ensure_initial_number_option(options: &mut Vec<NumberOption>, initial: f64) {
    if !options.iter().any(|o| o.value == initial) {
        options.insert(
            0,
            NumberOption {
                name: format_number_csg(initial),
                value: initial,
            },
        );
    }
}

fn parse_string_bracket(initial: &str, inner: &str) -> Option<ParamConstraint> {
    if inner.contains(',') {
        let mut options = Vec::new();
        for item in inner.split(',') {
            options.push(parse_string_option(item)?);
        }
        ensure_initial_string_option(&mut options, initial);
        return Some(ParamConstraint::StringEnum { options });
    }

    let parts: Vec<&str> = inner.split(':').collect();
    match parts.as_slice() {
        [only] => {
            let val = strip_optional_quotes(only.trim());
            let mut options = vec![StringOption {
                name: val.to_string(),
                value: val.to_string(),
            }];
            ensure_initial_string_option(&mut options, initial);
            Some(ParamConstraint::StringEnum { options })
        }
        [a, b] => {
            // If both sides parse as numbers (`[10:20]`), OpenSCAD parses the
            // bracket as a numeric range and ignores it for a string parameter.
            if parse_plain_f64(a).is_some() && parse_plain_f64(b).is_some() {
                return None;
            }
            let value = strip_optional_quotes(a.trim());
            let label = strip_optional_quotes(b.trim());
            let mut options = vec![StringOption {
                name: label.to_string(),
                value: value.to_string(),
            }];
            ensure_initial_string_option(&mut options, initial);
            Some(ParamConstraint::StringEnum { options })
        }
        _ => None,
    }
}

fn parse_string_option(item: &str) -> Option<StringOption> {
    let trimmed = item.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some((val_raw, label_raw)) = trimmed.split_once(':') {
        let value = strip_optional_quotes(val_raw.trim());
        let label = strip_optional_quotes(label_raw.trim());
        Some(StringOption {
            name: label.to_string(),
            value: value.to_string(),
        })
    } else {
        let value = strip_optional_quotes(trimmed);
        Some(StringOption {
            name: value.to_string(),
            value: value.to_string(),
        })
    }
}

fn ensure_initial_string_option(options: &mut Vec<StringOption>, initial: &str) {
    if !options.iter().any(|o| o.value == initial) {
        options.insert(
            0,
            StringOption {
                name: initial.to_string(),
                value: initial.to_string(),
            },
        );
    }
}

fn strip_optional_quotes(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(s)
}

fn parse_vector_bracket(
    param_name: &str,
    initial: &[f64],
    inner: &str,
    line: usize,
    warnings: &mut Vec<ParamWarning>,
) -> Option<VectorRange> {
    if inner.contains(',') {
        return None;
    }
    let parts: Vec<&str> = inner.split(':').collect();
    match parts.as_slice() {
        [only] => {
            let mut max = parse_plain_f64(only)?;
            let min = 0.0;
            let elem_max = initial.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            if elem_max > max {
                warnings.push(ParamWarning {
                    name: param_name.to_string(),
                    value: elem_max,
                    declared_bound: max,
                    kind: ParamWarningKind::AboveMaximum,
                    line,
                });
                max = elem_max;
            }
            Some(VectorRange {
                min,
                max,
                step: 1.0,
            })
        }
        [a, b] => {
            let raw_min = parse_plain_f64(a)?;
            let raw_max = parse_plain_f64(b)?;
            let (min, max) =
                adjust_vector_bounds(param_name, initial, raw_min, raw_max, line, warnings);
            Some(VectorRange {
                min,
                max,
                step: 1.0,
            })
        }
        [a, s, b] => {
            let raw_min = parse_plain_f64(a)?;
            let step = parse_plain_f64(s)?;
            let raw_max = parse_plain_f64(b)?;
            let (min, max) =
                adjust_vector_bounds(param_name, initial, raw_min, raw_max, line, warnings);
            Some(VectorRange { min, max, step })
        }
        _ => None,
    }
}

fn adjust_scalar_bounds(
    param_name: &str,
    initial: f64,
    mut min: f64,
    mut max: f64,
    line: usize,
    warnings: &mut Vec<ParamWarning>,
) -> (f64, f64) {
    if initial < min {
        warnings.push(ParamWarning {
            name: param_name.to_string(),
            value: initial,
            declared_bound: min,
            kind: ParamWarningKind::BelowMinimum,
            line,
        });
        min = initial;
    }
    if initial > max {
        warnings.push(ParamWarning {
            name: param_name.to_string(),
            value: initial,
            declared_bound: max,
            kind: ParamWarningKind::AboveMaximum,
            line,
        });
        max = initial;
    }
    (min, max)
}

fn adjust_vector_bounds(
    param_name: &str,
    initial: &[f64],
    mut min: f64,
    mut max: f64,
    line: usize,
    warnings: &mut Vec<ParamWarning>,
) -> (f64, f64) {
    let elem_min = initial.iter().copied().fold(f64::INFINITY, f64::min);
    let elem_max = initial.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if elem_min < min {
        warnings.push(ParamWarning {
            name: param_name.to_string(),
            value: elem_min,
            declared_bound: min,
            kind: ParamWarningKind::BelowMinimum,
            line,
        });
        min = elem_min;
    }
    if elem_max > max {
        warnings.push(ParamWarning {
            name: param_name.to_string(),
            value: elem_max,
            declared_bound: max,
            kind: ParamWarningKind::AboveMaximum,
            line,
        });
        max = elem_max;
    }
    (min, max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::to_param_json;

    #[test]
    fn porta_carte_and_separatore_match_openscad_param_json() {
        let porta_src = include_str!("../../../examples/porta_carte.scad");
        let porta = extract_parameters(porta_src);
        assert!(porta.warnings.is_empty());
        assert_eq!(
            to_param_json("porta_carte", &porta.parameters),
            r#"{"parameters":[{"caption":"taratura = dima per scegliere SERRAGGIO: 5 anelli di scatola e 1 di coperchio","group":"Pezzo","initial":"stampa","name":"PEZZO","options":[{"name":"stampa","value":"stampa"},{"name":"scatola","value":"scatola"},{"name":"coperchio","value":"coperchio"},{"name":"assieme","value":"assieme"},{"name":"taratura","value":"taratura"}],"type":"string"},{"group":"Configurazione","initial":"orizzontale","name":"ORIENTAMENTO","options":[{"name":"orizzontale","value":"orizzontale"},{"name":"verticale","value":"verticale"}],"type":"string"},{"group":"Configurazione","initial":1.0,"max":4.0,"min":1.0,"name":"N_FILE","step":1.0,"type":"number"},{"caption":"lunghezza massima in mm, arrotondata per difetto al passo da 5","group":"Configurazione","initial":70.0,"max":270.0,"min":30.0,"name":"LUNGHEZZA","step":1.0,"type":"number"},{"caption":"un separatore infilato storto resta piu' alto e traballa","group":"Configurazione","initial":true,"name":"PIEDINO","type":"boolean"},{"group":"Configurazione","initial":12.0,"name":"TAB_H","step":1.0,"type":"number"},{"caption":"con PEZZO = \"taratura\" (anello che chiude con un leggero attrito)","group":"Coperchio","initial":0.3,"max":0.6,"min":-0.25,"name":"SERRAGGIO","step":0.05,"type":"number"}],"title":"porta_carte"}"#
        );

        let sep_src = include_str!("../../../examples/separatore.scad");
        let sep = extract_parameters(sep_src);
        assert!(sep.warnings.is_empty());
        assert_eq!(
            to_param_json("separatore", &sep.parameters),
            r#"{"parameters":[{"group":"Separatore","initial":"orizzontale","name":"ORIENTAMENTO","options":[{"name":"orizzontale","value":"orizzontale"},{"name":"verticale","value":"verticale"}],"type":"string"},{"caption":"se il separatore e' infilato storto il piedino appoggia sul fondo e lui resta alto","group":"Separatore","initial":true,"name":"PIEDINO","type":"boolean"},{"group":"Separatore","initial":12.0,"name":"TAB_H","step":1.0,"type":"number"},{"caption":"posizioni della linguetta a cartella: 3 = terzi, 2 = meta'","group":"Separatore","initial":3.0,"max":4.0,"min":1.0,"name":"N_POS","step":1.0,"type":"number"},{"caption":"posizione di questo separatore: 0 = sinistra","group":"Separatore","initial":0.0,"max":3.0,"min":0.0,"name":"TAB_POS","step":1.0,"type":"number"},{"group":"Separatore","initial":"ROSSO","name":"TESTO","type":"string"},{"caption":"piena, aperta, x = diagonali, ruota = pentagono con cinque raggi","group":"Separatore","initial":"x","name":"FINESTRA","options":[{"name":"piena","value":"piena"},{"name":"aperta","value":"aperta"},{"name":"x","value":"x"},{"name":"ruota","value":"ruota"}],"type":"string"}],"title":"separatore"}"#
        );
    }

    #[test]
    fn cutoff_reassignment_warnings_and_defines_match_openscad() {
        let src = "\
dup = 1; // [0:10]
b = 25; // [0:10]
dup = 3; // [0:2:6]
/* plain block */
plain_grp = \"ok\";
/* [Hidden] */
sec = 9;
/* [After] */
str_brace = \"{cuts}\";
never_seen = 1;
";
        let extracted = extract_parameters(src);
        assert_eq!(extracted.warnings.len(), 1);
        assert_eq!(
            extracted.warnings[0].format_openscad("test.scad"),
            "WARNING: Parameter \"b\": value 25 is above the declared maximum 10, adjusting maximum value in file test.scad, line 2"
        );
        let names: Vec<&str> = extracted
            .parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["dup", "b", "plain_grp"]);
        assert_eq!(extracted.parameters[0].default, ParamValue::Number(3.0));
        assert_eq!(extracted.parameters[2].group, None);

        // With a `{` in the file, `-D` appends after `{` and removes `b` from parameters.
        let with_def = extract_parameters_with_defines(src, &[("b".into(), "5".into())]);
        let names_def: Vec<&str> = with_def
            .parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names_def, ["dup", "plain_grp"]);
    }
}
