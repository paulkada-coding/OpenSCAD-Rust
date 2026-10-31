//! Prints expressions the way OpenSCAD does when it shows a function value
//! or a failed assertion: binary and ternary operators are parenthesized,
//! string literals are escaped.

use std::fmt::Write as _;

use scad_syntax::ast::*;

use crate::value::format_number;

pub fn expr_to_string(expr: &Expr) -> String {
    let mut out = String::new();
    write_expr(expr, &mut out);
    out
}

pub fn params_to_string(params: &[Param]) -> String {
    let mut out = String::new();
    write_params(params, &mut out);
    out
}

fn write_params(params: &[Param], out: &mut String) {
    for (i, param) in params.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&param.name);
        if let Some(default) = &param.default {
            out.push_str(" = ");
            write_expr(default, out);
        }
    }
}

fn write_args(args: &[Argument], separator: &str, out: &mut String) {
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            out.push_str(separator);
        }
        if let Some(name) = &arg.name {
            out.push_str(name);
            out.push_str(" = ");
        }
        write_expr(&arg.value, out);
    }
}

fn write_string_literal(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn binary_symbol(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Mod => "%",
        BinaryOp::Pow => "^",
        BinaryOp::Lt => "<",
        BinaryOp::Le => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Ge => ">=",
        BinaryOp::Eq => "==",
        BinaryOp::Ne => "!=",
        BinaryOp::And => "&&",
        BinaryOp::Or => "||",
        BinaryOp::BitAnd => "&",
        BinaryOp::BitOr => "|",
        BinaryOp::Shl => "<<",
        BinaryOp::Shr => ">>",
    }
}

fn write_expr(expr: &Expr, out: &mut String) {
    match &expr.kind {
        ExprKind::Undef => out.push_str("undef"),
        ExprKind::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        ExprKind::Number(n) => out.push_str(&format_number(*n)),
        ExprKind::String(s) => write_string_literal(s, out),
        ExprKind::Variable(name) => out.push_str(name),
        ExprKind::Unary { op, operand } => match (op, &operand.kind) {
            // A negated literal is folded into the literal.
            (UnaryOp::Neg, ExprKind::Number(n)) => out.push_str(&format_number(-n)),
            (UnaryOp::Plus, _) => write_expr(operand, out),
            _ => {
                out.push_str(match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                    UnaryOp::BitNot => "~",
                    UnaryOp::Plus => "",
                });
                write_expr(operand, out);
            }
        },
        ExprKind::Binary { op, lhs, rhs } => {
            out.push('(');
            write_expr(lhs, out);
            let _ = write!(out, " {} ", binary_symbol(*op));
            write_expr(rhs, out);
            out.push(')');
        }
        ExprKind::Ternary {
            condition,
            then_expr,
            else_expr,
        } => {
            out.push('(');
            write_expr(condition, out);
            out.push_str(" ? ");
            write_expr(then_expr, out);
            out.push_str(" : ");
            write_expr(else_expr, out);
            out.push(')');
        }
        ExprKind::Call { callee, args } => {
            if matches!(callee.kind, ExprKind::Variable(_)) {
                write_expr(callee, out);
            } else {
                out.push('(');
                write_expr(callee, out);
                out.push(')');
            }
            out.push('(');
            write_args(args, ", ", out);
            out.push(')');
        }
        ExprKind::Index { target, index } => {
            write_expr(target, out);
            out.push('[');
            write_expr(index, out);
            out.push(']');
        }
        ExprKind::Member { target, member } => {
            write_expr(target, out);
            out.push('.');
            out.push_str(member);
        }
        ExprKind::Range { start, step, end } => {
            out.push('[');
            write_expr(start, out);
            if let Some(step) = step {
                out.push_str(" : ");
                write_expr(step, out);
            }
            out.push_str(" : ");
            write_expr(end, out);
            out.push(']');
        }
        ExprKind::List(elements) => {
            out.push('[');
            for (i, element) in elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_element(element, out);
            }
            out.push(']');
        }
        ExprKind::Let { assignments, body } => {
            out.push_str("let(");
            write_args(assignments, ", ", out);
            out.push_str(") ");
            write_expr(body, out);
        }
        ExprKind::Assert { args, body } | ExprKind::Echo { args, body } => {
            out.push_str(if matches!(expr.kind, ExprKind::Assert { .. }) {
                "assert("
            } else {
                "echo("
            });
            write_args(args, ", ", out);
            out.push(')');
            if let Some(body) = body {
                out.push(' ');
                write_expr(body, out);
            }
        }
        ExprKind::FunctionLiteral(literal) => {
            out.push_str("function(");
            write_params(&literal.params, out);
            out.push_str(") ");
            write_expr(&literal.body, out);
        }
    }
}

/// Generator bodies are wrapped in parentheses, except in C-style `for`.
fn write_element(element: &ListElement, out: &mut String) {
    let wrapped = |element: &ListElement, out: &mut String| {
        out.push('(');
        write_element(element, out);
        out.push(')');
    };
    match &element.kind {
        ListElementKind::Expr(expr) => write_expr(expr, out),
        ListElementKind::Each(inner) => {
            out.push_str("each ");
            wrapped(inner, out);
        }
        ListElementKind::For { assignments, body } => {
            out.push_str("for(");
            write_args(assignments, ", ", out);
            out.push_str(") ");
            wrapped(body, out);
        }
        ListElementKind::ForC {
            init,
            condition,
            update,
            body,
        } => {
            out.push_str("for(");
            write_args(init, ", ", out);
            out.push(';');
            write_expr(condition, out);
            out.push(';');
            write_args(update, ", ", out);
            out.push_str(") ");
            write_element(body, out);
        }
        ListElementKind::If {
            condition,
            then_element,
            else_element,
        } => {
            out.push_str("if(");
            write_expr(condition, out);
            out.push_str(") ");
            wrapped(then_element, out);
            if let Some(else_element) = else_element {
                out.push_str(" else ");
                wrapped(else_element, out);
            }
        }
        ListElementKind::Let { assignments, body } => {
            out.push_str("let(");
            write_args(assignments, ", ", out);
            out.push_str(") ");
            wrapped(body, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn printed(source: &str) -> String {
        let parse = scad_syntax::parse(&format!("x = {source};"));
        let StatementKind::Assignment(a) = &parse.tree().statements[0].kind else {
            panic!()
        };
        expr_to_string(&a.value)
    }

    #[test]
    fn matches_openscad() {
        let cases = [
            ("let(x = 1, y = 2) x + y", "let(x = 1, y = 2) (x + y)"),
            (
                "[for (i = [0:2:10]) if (i > 2) i else -i]",
                "[for(i = [0 : 2 : 10]) (if((i > 2)) (i) else (-i))]",
            ),
            (
                "[for (i = [0:2], j = [1]) let(k = i) each [k, j]]",
                "[for(i = [0 : 2], j = [1]) (let(k = i) (each ([k, j])))]",
            ),
            ("[for (i = 0; i < 3; i = i + 1) i]", "[for(i = 0;(i < 3);i = (i + 1)) i]"),
            ("v[0].x + v.y[1]", "(v[0].x + v.y[1])"),
            ("f(1, b = 2)(3)", "(f(1, b = 2))(3)"),
            ("a ? b ? 1 : 2 : c ? 3 : 4", "(a ? (b ? 1 : 2) : (c ? 3 : 4))"),
            ("-x + !y - -1 + +3", "(((-x + !y) - -1) + 3)"),
            ("-(-x)", "--x"),
            ("\"t\\tu\\\\\"", "\"t\\tu\\\\\""),
            ("1 | 2 & 3 << 4 >> 5 + ~6", "(1 | (2 & ((3 << 4) >> (5 + ~6))))"),
        ];
        for (source, expected) in cases {
            assert_eq!(printed(source), expected, "{source}");
        }
    }
}
